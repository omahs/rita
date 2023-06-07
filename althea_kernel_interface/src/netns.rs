use crate::KernelInterface;

impl dyn KernelInterface {
    /// Custom function for our integration test environment, returns a numbered netnamespace
    /// this thread is currently operating in, allowing us to dispatch lazy static data
    /// independent to each thread. Arch wise the fact that we have this problem at all indicates
    /// that the lazy static for cross thread comms arch if a bit questionable by nature
    pub fn check_integration_test_netns(&self) -> u32 {
        if cfg!(feature = "integration_test") {
            let ns = self.run_command("ip", &["netns", "identify"]).unwrap();
            let ns = match String::from_utf8(ns.stdout) {
                Ok(s) => s,
                Err(_) => panic!("Could not get netns name!"),
            };
            let ns = ns.trim();
            match (
                ns.split('-').last().unwrap().parse(),
                ns.split('_').last().unwrap().parse(),
            ) {
                (Ok(a), _) => a,
                (_, Ok(a)) => a,
                (Err(_), Err(_)) => {
                    // for some reason it's not easily possible to tell if we're in a unit test
                    error!("Could not get netns name, maybe a unit test?");
                    0
                }
            }
        } else {
            0
        }
    }
}
