linux-example example *args:
     cargo build --example {{example}} && \
     sudo setcap cap_net_raw=pe ./target/debug/examples/{{example}} && \
     ./target/debug/examples/{{example}} {{args}}

linux-example-release example *args:
     cargo build --example {{example}} --release && \
     sudo setcap cap_net_raw=pe ./target/release/examples/{{example}} && \
     ./target/release/examples/{{example}} {{args}}
