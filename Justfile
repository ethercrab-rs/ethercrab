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
