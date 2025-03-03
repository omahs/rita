#!/bin/bash
# Number of validators to start
NODES=$1
# what test to execute
TEST_TYPE=$2
set -eux

# Stop any currently running peggy and eth processes
pkill althea || true # allowed to fail

# Wipe filesystem changes
for i in $(seq 1 $NODES);
do
    rm -rf "/validator$i"
done

/althea_rs/scripts/integration_tests/container_scripts/setup-validators.sh $NODES
/althea_rs/scripts/integration_tests/container_scripts/run-testnet.sh $NODES

sleep 10

echo "Althea chain now running in the background!"

# This keeps the script open to prevent Docker from stopping the container
# immediately if the nodes are killed by a different process
read -p "Press Return to Close..."