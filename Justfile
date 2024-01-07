linux-example example *args:
    cargo build --example {{example}} && \
    sudo setcap cap_net_raw=pe ./target/debug/examples/{{example}} && \
    ./target/debug/examples/{{example}} {{args}}

linux-example-release example *args:
    cargo build --example {{example}} --release && \
    sudo setcap cap_net_raw=pe ./target/release/examples/{{example}} && \
    ./target/release/examples/{{example}} {{args}}

linux-test *args:
    #!/usr/bin/env bash

    set -e

    OUT=$(cargo test --features '__internals' --no-run 2>&1 | tee /dev/tty | grep -oE '\(target/.+\)' | sed 's/[)(]//g')
    # BINS=$(echo $OUT)

    mapfile -t BINS < <( echo "$OUT" )

    for BIN in "${BINS[@]}"
    do
        echo "  Setcap for test binary $BIN"
        sudo setcap cap_net_raw=pe $BIN
    done

    # We've now setcap'd everything so we should be able to run this again without perm issues
    cargo test --features '__internals' {{args}}

linux-bench *args:
     cargo bench --features __internals {{args}}
     sudo echo
     fd . --type executable ./target/release/deps -x sudo setcap cap_net_raw=pe
     cargo bench --features __internals {{args}}

_generate-readme path:
     cargo readme --project-root "{{path}}" --template README.tpl --output README.md
     # Remove unprocessed doc links
     sed -i 's/\[\(`[^`]*`\)] /\1 /g' README.md

_check-readme path: (_generate-readme path)
     git diff --quiet --exit-code "{{path}}/README.md"

check-readmes: (_check-readme ".") (_check-readme "./ethercrab-wire") (_check-readme "./ethercrab-wire-derive")

generate-readmes: (_generate-readme ".") (_generate-readme "./ethercrab-wire") (_generate-readme "./ethercrab-wire-derive")

dump-eeprom *args:
    cargo build --example dump-eeprom --features "std __internals" --release && \
    sudo setcap cap_net_raw=pe ./target/release/examples/dump-eeprom && \
    ./target/release/examples/dump-eeprom {{args}}

test-replay test_file *args:
    cargo test --features '__internals' {{ replace(test_file, '-', '_') }}

capture-replay test_file_name interface *args:
    #!/usr/bin/env bash

    set -euo pipefail

    # Kill child tshark on failure
    trap 'kill 0' EXIT

    test_file=$(echo "{{test_file_name}}" | tr '_' '-')

    if [ ! -f "tests/${test_file}.rs" ]; then
        echo "Test file tests/${test_file}.rs does not exist"

        exit 1
    fi

    cargo build --features '__internals' --tests --release
    sudo echo
    fd . --type executable ./target/debug/deps -x sudo setcap cap_net_raw=pe
    fd . --type executable ./target/release -x sudo setcap cap_net_raw=pe

    tshark -Q -w "tests/${test_file}.pcapng" --interface {{interface}} -f 'ether proto 0x88a4' &

    # Wait for tshark to start
    sleep 1

    test_name=$(echo "${test_file}" | tr '-' '_')

    # Set env var to put test in capture mode
    INTERFACE="{{interface}}" cargo test "${test_name}" --release --features '__internals' -- {{args}}

    # Let tshark finish up
    sleep 1

    # Kill tshark
    kill 0
