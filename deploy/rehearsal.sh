#!/usr/bin/env bash
# A rehearsal network: four validators spread over several machines.
#
#   deploy/rehearsal.sh create     build the programs and create the network files in net/
#   deploy/rehearsal.sh push       copy the program, genesis file and one key to each machine
#   deploy/rehearsal.sh start      start the validators on the other machines
#   deploy/rehearsal.sh local      run this machine's validator here, in the foreground
#   deploy/rehearsal.sh status     ask every validator how it is doing
#   deploy/rehearsal.sh stop       stop the validators on the other machines
#   deploy/rehearsal.sh remove     stop them and delete their folders
#
# start and stop take validator names to act on some only: `stop v3`.
#
# Connections are not encrypted, so every address below must be a private
# one (these are Tailscale addresses). Nothing is installed on the other
# machines and nothing there survives a reboot: each gets one folder and one
# process. Keys and wallets live in net/, which git ignores.

set -euo pipefail
cd "$(dirname "$0")/.."

NET=net
NAME=orien-rehearsal
LINUX=x86_64-unknown-linux-gnu
ORIEN=target/release/orien
ORIEN_LINUX=target/$LINUX/release/orien

# validator | where ("local" is this machine) | folder there | address | run as (user:group, or -)
#
# The always-on machine holds two validators, so the network keeps working
# while either of the other two is asleep or switched off.
LAYOUT="
v1 local                            -                      100.76.239.128:7101   -
v2 christopher@christopher-desktop  /home/christopher/orien 100.103.169.102:7101  -
v3 root@cyphers-vps                 /opt/orien             100.99.59.98:7101     nobody:nogroup
v4 root@cyphers-vps                 /opt/orien             100.99.59.98:7102     nobody:nogroup
"

SSH="ssh -o BatchMode=yes -o ConnectTimeout=10"

# Calls "$1 name where folder address run_as" for each validator, or only
# for those named in the remaining arguments.
each() {
    local action=$1; shift
    local name where folder address run_as
    # The list is read on its own channel: ssh would otherwise swallow it.
    while read -r name where folder address run_as <&3; do
        [ -n "$name" ] || continue
        if [ $# -gt 0 ] && ! printf '%s\n' "$@" | grep -qx "$name"; then continue; fi
        "$action" "$name" "$where" "$folder" "$address" "$run_as"
    done 3<<< "$LAYOUT"
}

# The pattern that picks out one validator's process and nothing else.
process() { printf '^%s/orien node .*/%s/%s\\.key' "$1" "$2" "$2"; }

create() {
    [ ! -e $NET/genesis.json ] || { echo "$NET/ already holds a network; delete it to start over"; exit 1; }
    cargo build --release -p orien-cli
    cargo zigbuild --release --target $LINUX.2.31 -p orien-cli
    local addresses=()
    add() { addresses+=(--address "$4"); }
    each add
    $ORIEN devnet init --dir $NET --name $NAME "${addresses[@]}"
}

push_one() {
    local name=$1 where=$2 folder=$3 run_as=$5
    [ "$where" != local ] || return 0
    local owner=""
    [ "$run_as" = - ] || owner="-o ${run_as%:*} -g ${run_as#*:}"
    echo "$name -> $where:$folder"
    $SSH "$where" "mkdir -p '$folder' && install -d -m 700 $owner '$folder/$name' '$folder/$name/data'"
    scp -q $ORIEN_LINUX "$where:$folder/orien.new"
    scp -q $NET/genesis.json "$where:$folder/genesis.json"
    scp -q $NET/validators/$name.key "$where:$folder/$name/$name.key.new"
    # Swap the program in whole, so a running validator is never half-overwritten.
    $SSH "$where" "chmod 755 '$folder/orien.new' && mv '$folder/orien.new' '$folder/orien' \
        && install -m 600 $owner '$folder/$name/$name.key.new' '$folder/$name/$name.key' \
        && rm '$folder/$name/$name.key.new'"
}

start_one() {
    local name=$1 where=$2 folder=$3 address=$4 run_as=$5
    if [ "$where" = local ]; then
        echo "$name runs on this machine: deploy/rehearsal.sh local"
        return 0
    fi
    local drop=""
    [ "$run_as" = - ] || drop="setpriv --reuid=${run_as%:*} --regid=${run_as#*:} --clear-groups"
    local command="$folder/orien node -g $folder/genesis.json -k $folder/$name/$name.key -d $folder/$name/data --listen $address"
    $SSH "$where" "cd '$folder/$name' || exit 1
        if pgrep -f '$(process "$folder" "$name")' > /dev/null; then echo '$name is already running'; exit 0; fi
        # Keep the previous run's log aside, so the check below reads only this run.
        [ ! -f node.log ] || mv node.log node.log.1
        setsid $drop nohup $command > node.log 2>&1 < /dev/null &
        for _ in 1 2 3 4 5 6 7 8 9 10; do
            grep -q 'is listening' node.log && { echo '$name started on $address'; exit 0; }
            sleep 1
        done
        echo '$name did not start:'; tail -5 node.log; exit 1"
}

stop_one() {
    local name=$1 where=$2 folder=$3
    [ "$where" != local ] || return 0
    $SSH "$where" "pkill -f '$(process "$folder" "$name")' && echo '$name stopped' || echo '$name was not running'"
}

remove_one() {
    local name=$1 where=$2 folder=$3
    [ "$where" != local ] || return 0
    stop_one "$@"
    case "$folder" in
        */orien) $SSH "$where" "rm -rf '$folder/$name' && rmdir '$folder' 2>/dev/null || true" ;;
        *) echo "refusing to delete $folder: it is not a folder this script made"; exit 1 ;;
    esac
}

run_local() {
    local name=$1 where=$2 address=$4
    [ "$where" = local ] || return 0
    exec $ORIEN node -g $NET/genesis.json -k $NET/validators/$name.key -d $NET/data/$name --listen "$address"
}

case "${1:-}" in
    create) create ;;
    push)   each push_one ;;
    start)  shift; each start_one "$@" ;;
    local)  each run_local ;;
    status) ORIEN_GENESIS=$NET/genesis.json $ORIEN status ;;
    stop)   shift; each stop_one "$@" ;;
    remove) each remove_one ;;
    *)      sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
