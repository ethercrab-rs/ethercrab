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

linux-bench *args:
     cargo bench --features bench-hacks --no-run {{args}}
     sudo echo
     fd . --type executable ./target/release/deps -x sudo setcap cap_net_raw=pe
     cargo bench --features bench-hacks {{args}}

generate-readme:
     cargo readme > README.md
     # Remove unprocessed doc links
     sed -i 's/\[\(`[^`]*`\)] /\1 /g' README.md

check-readme: (generate-readme)
     git diff --quiet --exit-code README.md
