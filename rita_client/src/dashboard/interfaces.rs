//! A generalized interface for modifying networking interface assignments using UCI
use actix_web::Path;
use actix_web::{HttpRequest, HttpResponse, Json};
use failure::bail;
use failure::Error;
use rita_common::peer_listener::unlisten_interface;
use rita_common::KI;
use settings::FileWrite;
use std::collections::HashMap;
use std::net::Ipv4Addr;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InterfaceToSet {
    pub interface: String,
    pub mode: InterfaceMode,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Copy)]
pub enum InterfaceMode {
    /// 'Mesh' mode essentially defines a port where Rita is attached and performing
    /// it's own hello/ImHere protocol as defined in PeerListener. These ports are just
    /// setup as static in the OpenWRT config and so long as SLACC IPv6 linklocal auto
    /// negotiation is on Rita takes it from there. Some key caveats is that a port
    /// may not be 'mesh' (eg: defined in Rita's peer_interfaces var) and also configured
    /// as a WAN port.
    Mesh,
    /// LAN port is essentially defined as any port attached to the br-lan bridge. The br-lan
    /// bridge then provides DHCP. Finally Rita comes in and places a default route though the
    /// exit server so that users can actually reach the internet.
    Lan,
    /// WAN port, specifically means that this device is listening for DHCP (instead of providing DHCP
    /// like on LAN). This route will be accepted and installed. Keep in mind if a WAN port is set this
    /// device will count itself as a 'gateway' regardless of if there is actual activity on the port.
    /// Example effects of a device being defined as a 'gateway' include making DHCP requests for manual_peers
    /// in tunnel_manager and taking the gateway price in operator_update
    Wan,
    /// Same as WAN but configures a static IP for this device.
    StaticWan {
        netmask: Ipv4Addr,
        ipaddr: Ipv4Addr,
        gateway: Ipv4Addr,
    },
    /// This represents a port dedicated to phone network extending antennas. This is an extension of the
    /// AltheaMobile SSID and can be boiled down to attaching the port to br-pbs over which devices will
    /// then be assigned phone network DHCP and IPs
    Phone,
    /// Ambiguous wireless modes like monitor, or promiscuous show up here, but other things that might also
    /// be unknown are various forms of malformed configs. Take for example a StaticWAN missing a config param
    Unknown,
}

impl ToString for InterfaceMode {
    fn to_string(&self) -> String {
        match self {
            InterfaceMode::Mesh => "mesh".to_owned(),
            InterfaceMode::Lan => "LAN".to_owned(),
            InterfaceMode::Wan => "WAN".to_owned(),
            InterfaceMode::StaticWan { .. } => "StaticWAN".to_owned(),
            InterfaceMode::Phone => "Phone".to_owned(),
            InterfaceMode::Unknown => "unknown".to_owned(),
        }
    }
}

/// Gets a list of interfaces and their modes by parsing UCI
pub fn get_interfaces() -> Result<HashMap<String, InterfaceMode>, Error> {
    let mut retval = HashMap::new();

    // Wired
    for (setting_name, value) in KI.uci_show(Some("network"))? {
        // Only non-loopback non-bridge interface names should get past
        if setting_name.contains("ifname") && !value.contains("backhaul") && value != "lo" {
            // it's a list and we need to handle that
            if value.contains(' ') {
                for list_member in value.split(' ') {
                    if list_member.contains("pbs-wlan") {
                        continue;
                    }
                    retval.insert(
                        list_member.replace(" ", "").to_string(),
                        ethernet2mode(&value, &setting_name)?,
                    );
                }
            } else {
                if value.contains("pbs-wlan") {
                    continue;
                }
                retval.insert(value.clone(), ethernet2mode(&value, &setting_name)?);
            }
        }
    }

    Ok(retval)
}

/// Find out a wired interface's mode (mesh, LAN, WAN) from the setting name
pub fn ethernet2mode(ifname: &str, setting_name: &str) -> Result<InterfaceMode, Error> {
    trace!(
        "ethernet2mode: ifname {:?}, setting_name {:?}",
        ifname,
        setting_name
    );

    // Match parent section name
    Ok(match &setting_name.replace(".ifname", "") {
        s if s.contains("rita_") => InterfaceMode::Mesh,
        s if s.contains("lan") => InterfaceMode::Lan,
        s if s.contains("pbs") => InterfaceMode::Phone,
        s if s.contains("backhaul") => {
            let prefix = "network.backhaul";
            let backhaul = KI.uci_show(Some(prefix))?;
            trace!("{:?}", backhaul);
            let proto = if let Some(val) = backhaul.get(&format!("{}.proto", prefix)) {
                val
            } else {
                bail!("WAN network with no proto?");
            };

            if proto.contains("dhcp") {
                return Ok(InterfaceMode::Wan);
            } else if proto.contains("static") {
                let opt_tuple = (
                    backhaul.get(&format!("{}.netmask", prefix)),
                    backhaul.get(&format!("{}.ipaddr", prefix)),
                    backhaul.get(&format!("{}.gateway", prefix)),
                );
                if let (Some(netmask), Some(ipaddr), Some(gateway)) = opt_tuple {
                    return Ok(InterfaceMode::StaticWan {
                        netmask: netmask.parse()?,
                        ipaddr: ipaddr.parse()?,
                        gateway: gateway.parse()?,
                    });
                } else {
                    bail!("Failed to parse static wan!");
                }
            }
            bail!("Failed to parse backhaul entry!");
        }
        other => {
            warn!(
                "Unknown wired port mode for interface {:?}, section name {:?}",
                ifname, other
            );
            InterfaceMode::Unknown
        }
    })
}

fn set_interface_mode(iface_name: &str, mode: InterfaceMode) -> Result<(), Error> {
    trace!("InterfaceToSet recieved");
    let iface_name = iface_name;
    let target_mode = mode;
    let interfaces = get_interfaces()?;
    let current_mode = get_current_interface_mode(&interfaces, iface_name);
    if !interfaces.contains_key(iface_name) {
        bail!("Attempted to configure non-existant or unavailable interface!");
    } else if target_mode == InterfaceMode::Wan {
        // we can only have one WAN interface, check for others
        // StaticWAN entires are not identified seperately but if they ever are
        // you'll have to handle them here
        for entry in interfaces {
            let mode = entry.1;
            if mode == InterfaceMode::Wan {
                bail!("There can only be one WAN interface!");
            }
        }
    }

    trace!("Transforming ethernet");
    ethernet_transform_mode(iface_name, current_mode, target_mode)
}

/// Transform a wired inteface from mode A to mode B
pub fn ethernet_transform_mode(
    ifname: &str,
    a: InterfaceMode,
    b: InterfaceMode,
) -> Result<(), Error> {
    trace!(
        "Ethernet mode transform: ifname {:?}, a {:?}, b {:?}",
        ifname,
        a,
        b,
    );
    if a == b {
        // noop that was easy!
        return Ok(());
    } else if a == InterfaceMode::Unknown || b == InterfaceMode::Unknown {
        bail!("We can't change Unknown interfaces!");
    }
    let rita_client = settings::get_rita_client();
    let mut network = rita_client.network;

    // if we have edited UCI and it fails we set this var to handle cleanup later
    let mut return_codes = Vec::new();
    // in case of failure we revert to here
    let old_network_settings = { network.clone() };
    let filtered_ifname = format!("network.rita_{}", ifname.replace(".", ""));

    match a {
        // Wan is very simple, just delete it
        InterfaceMode::Wan | InterfaceMode::StaticWan { .. } => {
            network.external_nic = None;

            let ret = KI.del_uci_var("network.backhaul");
            return_codes.push(ret);
        }
        // LAN is a bridge and the lan bridge must always remain because things
        // like WiFi interfaces are attached to it. So we just remove the interface
        // from the list
        InterfaceMode::Lan => {
            let list = KI.get_uci_var("network.lan.ifname")?;
            let new_list = list_remove(&list, ifname);
            let ret = KI.set_uci_var("network.lan.ifname", &new_list);
            return_codes.push(ret);
        }
        // just like LAN we are adding and removing a device from the list just this time
        // on pbs
        InterfaceMode::Phone => {
            let list = KI.get_uci_var("network.pbs.ifname")?;
            let new_list = list_remove(&list, ifname);
            let ret = KI.set_uci_var("network.pbs.ifname", &new_list);
            return_codes.push(ret);
        }
        // for mesh we need to send an unlisten so that Rita stops
        // listening then we can remove the section, we also need to remove it
        // from the config
        InterfaceMode::Mesh => {
            unlisten_interface(ifname.to_string());
            network.peer_interfaces.remove(ifname);

            let ret = KI.del_uci_var(&filtered_ifname);
            return_codes.push(ret);
        }
        InterfaceMode::Unknown => unimplemented!(),
    }

    match b {
        // here we add back all the properties of backhaul we removed
        InterfaceMode::Wan => {
            network.external_nic = Some(ifname.to_string());

            let ret = KI.set_uci_var("network.backhaul", "interface");
            return_codes.push(ret);
            let ret = KI.set_uci_var("network.backhaul.ifname", ifname);
            return_codes.push(ret);
            let ret = KI.set_uci_var("network.backhaul.proto", "dhcp");
            return_codes.push(ret);
        }
        InterfaceMode::StaticWan {
            netmask,
            ipaddr,
            gateway,
        } => {
            network.external_nic = Some(ifname.to_string());

            let ret = KI.set_uci_var("network.backhaul", "interface");
            return_codes.push(ret);
            let ret = KI.set_uci_var("network.backhaul.ifname", ifname);
            return_codes.push(ret);
            let ret = KI.set_uci_var("network.backhaul.proto", "static");
            return_codes.push(ret);
            let ret = KI.set_uci_var("network.backhaul.netmask", &format!("{}", netmask));
            return_codes.push(ret);
            let ret = KI.set_uci_var("network.backhaul.ipaddr", &format!("{}", ipaddr));
            return_codes.push(ret);
            let ret = KI.set_uci_var("network.backhaul.gateway", &format!("{}", gateway));
            return_codes.push(ret);
        }
        // since we left lan mostly unmodified we just pop in the ifname
        InterfaceMode::Lan => {
            trace!("Converting interface to lan with ifname {:?}", ifname);
            let ret = KI.get_uci_var("network.lan.ifname");
            match ret {
                Ok(list) => {
                    trace!("The existing LAN interfaces list is {:?}", list);
                    let new_list = list_add(&list, ifname);
                    trace!("Setting the new list {:?}", new_list);
                    let ret = KI.set_uci_var("network.lan.ifname", &new_list);
                    return_codes.push(ret);
                }
                Err(e) => {
                    if e.to_string().contains("Entry not found") {
                        trace!("No LAN interfaces found, setting one now");
                        let ret = KI.set_uci_var("network.lan.ifname", ifname);
                        return_codes.push(ret);
                    } else {
                        warn!("Trying to read lan ifname returned {:?}", e);
                        return_codes.push(Err(e));
                    }
                }
            }
        }
        InterfaceMode::Phone => {
            trace!("Converting interface to Phone with ifname {:?}", ifname);
            let ret = KI.get_uci_var("network.pbs.ifname");
            match ret {
                Ok(list) => {
                    trace!("The existing Phone interfaces list is {:?}", list);
                    let new_list = list_add(&list, ifname);
                    trace!("Setting the new list {:?}", new_list);
                    let ret = KI.set_uci_var("network.pbs.ifname", &new_list);
                    return_codes.push(ret);
                }
                Err(e) => {
                    if e.to_string().contains("Entry not found") {
                        trace!("No Phone interfaces found, setting one now");
                        let ret = KI.set_uci_var("network.pbs.ifname", ifname);
                        return_codes.push(ret);
                    } else {
                        warn!("Trying to read Phone ifname returned {:?}", e);
                        return_codes.push(Err(e));
                    }
                }
            }
        }
        InterfaceMode::Mesh => {
            network.peer_interfaces.insert(ifname.to_string());

            let ret = KI.set_uci_var(&filtered_ifname, "interface");
            return_codes.push(ret);
            let ret = KI.set_uci_var(&format!("{}.ifname", filtered_ifname), ifname);
            return_codes.push(ret);
            let ret = KI.set_uci_var(&format!("{}.proto", filtered_ifname), "static");
            return_codes.push(ret);
        }
        InterfaceMode::Unknown => unimplemented!(),
    }

    // check all of our return codes in order to handle any possible issue
    let mut error_occured = false;
    for ret in return_codes {
        if ret.is_err() {
            error_occured = true;
        }
    }
    let mut rita_client = settings::get_rita_client();
    if error_occured {
        let res = KI.uci_revert("network");
        rita_client.network = old_network_settings;
        settings::set_rita_client(rita_client);
        bail!("Error running UCI commands! Revert attempted: {:?}", res);
    }

    KI.uci_commit("network")?;
    KI.openwrt_reset_network()?;

    rita_client.network = network;
    settings::set_rita_client(rita_client);

    // try and save the config and fail if we can't
    let rita_client = settings::get_rita_client();
    if let Err(_e) = rita_client.write(&settings::get_flag_config()) {
        return Err(_e);
    }

    // We edited disk contents, force global sync
    KI.fs_sync()?;

    trace!("Successsfully transformed ethernet mode, rebooting");
    KI.run_command("reboot", &[])?;

    Ok(())
}

/// Unlike physical ethernet interfaces you can run multiple SSID's on a single WIFI card
/// so we don't provide options to 'change' wireless modes to match the users expectations
/// instead we provide a toggle interface.
/// For example 'toggle user wlan off' or 'toggle phone sale network on' or 'toggle router
/// to router wireless meshing'.

fn wlan_toggle_get(uci_spec: &str) -> Result<bool, Error> {
    if !KI.is_openwrt() {
        bail!("Not an OpenWRT device!");
    }
    let bad_wireless = "Wireless config not correct";
    let current_state = KI.uci_show(Some(uci_spec))?;
    let current_state = match current_state.get(uci_spec) {
        Some(val) => val,
        None => bail!(bad_wireless),
    };
    let current_state = if current_state.contains('0') {
        true
    } else if current_state.contains('1') {
        false
    } else {
        bail!(bad_wireless);
    };

    trace!(
        "wlan get status: uci_spec {}, enabled: {}",
        uci_spec,
        current_state
    );

    Ok(current_state)
}

pub fn wlan_mesh_get(_: HttpRequest) -> HttpResponse {
    let res = wlan_toggle_get("wireless.mesh.disabled");
    match res {
        Ok(b) => HttpResponse::Ok().json(b),
        Err(e) => {
            error!("get mesh failed with {:?}", e);
            HttpResponse::InternalServerError().into()
        }
    }
}

pub fn wlan_lightclient_get(_: HttpRequest) -> HttpResponse {
    let res = wlan_toggle_get("wireless.lightclient.disabled");
    match res {
        Ok(b) => HttpResponse::Ok().json(b),
        Err(e) => {
            error!("get lightclient failed with {:?}", e);
            HttpResponse::InternalServerError().into()
        }
    }
}

fn wlan_toggle_set(uci_spec: &str, enabled: bool) -> Result<(), Error> {
    if !KI.is_openwrt() {
        bail!("Not an OpenWRT device!");
    }
    let bad_wireless = "Wireless config not correct";
    trace!("wlan toggle: uci_spec {}, enabled: {}", uci_spec, enabled,);

    let current_state = KI.uci_show(Some(uci_spec))?;
    let current_state = match current_state.get(uci_spec) {
        Some(val) => val,
        None => bail!(bad_wireless),
    };
    let current_state = if current_state.contains('0') {
        true
    } else if current_state.contains('1') {
        false
    } else {
        bail!(bad_wireless);
    };

    if enabled == current_state {
        return Ok(());
    }

    // remember it's a 'disabled' toggle so we want to set it to zero to be 'enabled'
    let res = if enabled {
        KI.set_uci_var(uci_spec, "0")
    } else {
        KI.set_uci_var(uci_spec, "1")
    };

    if let Err(e) = res {
        let res_b = KI.uci_revert("wireless");
        bail!(
            "Error running UCI commands! {:?} Revert attempted: {:?}",
            e,
            res_b
        );
    }

    KI.uci_commit("wireless")?;
    KI.openwrt_reset_wireless()?;

    // We edited disk contents, force global sync
    KI.fs_sync()?;

    KI.run_command("reboot", &[])?;

    Ok(())
}

pub fn wlan_mesh_set(enabled: Path<bool>) -> HttpResponse {
    let enabled = enabled.into_inner();
    let res = wlan_toggle_set("wireless.mesh.disabled", enabled);
    match res {
        Ok(_) => HttpResponse::Ok().into(),
        Err(e) => {
            error!("set mesh failed with {:?}", e);
            HttpResponse::InternalServerError().into()
        }
    }
}

pub fn wlan_lightclient_set(enabled: Path<bool>) -> HttpResponse {
    let enabled = enabled.into_inner();
    let res = wlan_toggle_set("wireless.lightclient.disabled", enabled);
    match res {
        Ok(_) => HttpResponse::Ok().into(),
        Err(e) => {
            error!("set lightclient failed with {:?}", e);
            HttpResponse::InternalServerError().into()
        }
    }
}

/// A helper function for adding entries to a list
pub fn list_add(list: &str, entry: &str) -> String {
    if !list.is_empty() {
        format!("{} {}", list, entry)
    } else {
        entry.to_string()
    }
}

/// A helper function for removing entries from a list
pub fn list_remove(list: &str, entry: &str) -> String {
    if !list.is_empty() {
        let split = list.split(' ');
        let mut new_list = "".to_string();
        let mut first = true;
        for item in split {
            let filtered_item = item.trim();
            if !item.contains(entry) {
                trace!("{} is not {} it's on the list!", filtered_item, entry);
                let tmp_list = new_list.to_string();
                if first {
                    new_list = tmp_list + &filtered_item.to_string();
                    first = false;
                } else {
                    new_list = tmp_list + &format!(" {}", filtered_item);
                }
            }
        }
        new_list
    } else {
        "".to_string()
    }
}

pub fn get_current_interface_mode(
    interfaces: &HashMap<String, InterfaceMode>,
    target_iface: &str,
) -> InterfaceMode {
    for entry in interfaces {
        let iface = entry.0;
        let mode = entry.1;
        if iface.contains(target_iface) {
            return *mode;
        }
    }
    InterfaceMode::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_remove() {
        let a = "eth0.3 eth1 eth2 eth3 eth4";

        let b = list_remove(a, "eth1");
        assert_eq!(b, "eth0.3 eth2 eth3 eth4");

        let b = list_remove(&b, "eth0.3");
        assert_eq!(b, "eth2 eth3 eth4");

        let b = list_remove(&b, "eth4");
        assert_eq!(b, "eth2 eth3");

        let b = list_remove(&b, "eth2");
        assert_eq!(b, "eth3");

        let b = list_remove(&b, "eth3");
        assert_eq!(b, "");
    }

    #[test]
    fn test_list_add() {
        let a = "";

        let b = list_add(a, "eth1");
        assert_eq!(b, "eth1");

        let b = list_add(&b, "eth0.3");
        assert_eq!(b, "eth1 eth0.3");

        let b = list_add(&b, "eth4");
        assert_eq!(b, "eth1 eth0.3 eth4");
    }
}

pub fn get_interfaces_endpoint(
    _req: HttpRequest,
) -> Result<Json<HashMap<String, InterfaceMode>>, Error> {
    debug!("get /interfaces hit");
    match get_interfaces() {
        Ok(val) => Ok(Json(val)),
        Err(e) => {
            error!("get_interfaces failed with {:?}", e);
            Err(e)
        }
    }
}

pub fn set_interfaces_endpoint(interface: Json<InterfaceToSet>) -> HttpResponse {
    let interface = interface.into_inner();
    debug!("set /interfaces hit");

    match set_interface_mode(&interface.interface, interface.mode) {
        Ok(_) => HttpResponse::Ok().into(),
        Err(e) => {
            error!("Set interfaces failed with {:?}", e);
            HttpResponse::InternalServerError().into()
        }
    }
}
