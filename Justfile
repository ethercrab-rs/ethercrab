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

    OUT=$(cargo test --no-run 2>&1 | tee /dev/tty | grep -oE '\(target/.+\)' | sed 's/[)(]//g')
    # BINS=$(echo $OUT)

    mapfile -t BINS < <( echo "$OUT" )

    for BIN in "${BINS[@]}"
    do
        echo "  Setcap for test binary $BIN"
        sudo setcap cap_net_raw=pe $BIN
    done

    # We've now setcap'd everything so we should be able to run this again without perm issues
    cargo test {{args}}

miri *args:
    MIRIFLAGS="-Zmiri-symbolic-alignment-check -Zmiri-isolation-error=warn -Zdeduplicate-diagnostics=yes" cargo +nightly-2025-03-29 miri test --target aarch64-unknown-linux-gnu {{args}}

miri-nextest *args:
    MIRIFLAGS="-Zmiri-symbolic-alignment-check -Zmiri-isolation-error=warn -Zdeduplicate-diagnostics=yes" cargo +nightly-2025-03-29 miri nextest run --target aarch64-unknown-linux-gnu {{args}}

_generate-readme path:
     cargo readme --project-root "{{path}}" --template README.tpl --output README.md
     # Remove unprocessed doc links
     sed -i 's/\[\(`[^`]*`\)] /\1 /g' README.md

_check-readme path: (_generate-readme path)
     git diff --quiet --exit-code "{{path}}/README.md"

check-readmes: (_check-readme ".") (_check-readme "./ethercrab-wire") (_check-readme "./ethercrab-wire-derive")

generate-readmes: (_generate-readme ".") (_generate-readme "./ethercrab-wire") (_generate-readme "./ethercrab-wire-derive")

dump-eeprom *args:
    cargo build --example dump-eeprom --features "std" --release && \
    sudo setcap cap_net_raw=pe ./target/release/examples/dump-eeprom && \
    ./target/release/examples/dump-eeprom {{args}}

test-replay test_file *args:
    cargo test {{ replace(test_file, '-', '_') }}

capture-replay test_name interface *args:
    #!/usr/bin/env bash

    set -euo pipefail

    # Kill child tshark on failure
    trap 'killall tshark' EXIT

    test_file=$(echo "{{test_name}}" | tr '_' '-')

    if [ ! -f "tests/${test_file}.rs" ]; then
        echo "Test file tests/${test_file}.rs does not exist"

        exit 1
    fi

    cargo build --tests --release
    sudo echo
    fd . --type executable ./target/debug/deps -x sudo setcap cap_net_raw=pe
    fd . --type executable ./target/release -x sudo setcap cap_net_raw=pe

    tshark -Q -w "tests/${test_file}.pcapng" --interface {{interface}} -f 'ether proto 0x88a4' &

    # Wait for tshark to start
    sleep 1

    test_name=$(echo "${test_file}" | tr '-' '_')

    # Set env var to put test in capture mode
    INTERFACE="{{interface}}" cargo test "${test_name}" --release -- {{args}}

    # Let tshark finish up
    sleep 1

    killall tshark

capture-all-replays interface *args:
    #!/usr/bin/env bash

    set -euo pipefail

    for file in `ls tests/replay-*.rs`; do
        test_name=$(basename "${file%.*}" | tr '-' '_')

        echo "Capturing $test_name, test is:"
        echo ""
        grep '//!' $file
        echo ""

        read -p "Prepare hardware then press any key to continue." </dev/tty

        just capture-replay ${test_name} {{interface}} {{args}}

        echo ""
    done
