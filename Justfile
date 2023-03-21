linux-example example *args:
     cargo build --example {{example}} && \
     sudo setcap cap_net_raw=pe ./target/debug/examples/{{example}} && \
     ./target/debug/examples/{{example}} {{args}}

linux-example-release example *args:
     cargo build --example {{example}} --release && \
     sudo setcap cap_net_raw=pe ./target/release/examples/{{example}} && \
     ./target/release/examples/{{example}} {{args}}

linux-test *args:
     #!/usr/bin/env sh

     set -e

     OUT=$(cargo test --no-run 2>&1 | tee /dev/tty | tail -n1)
     BIN=$(echo $OUT | awk -F '[\\(\\)]' '{print $2}')
     sudo setcap cap_net_raw=pe $BIN
     echo "$BIN {{args}}"
     $BIN {{args}}
