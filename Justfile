linux-example-release example:
     cargo build --example {{example}} --release && sudo setcap cap_net_raw=pe ./target/release/examples/{{example}} && RUST_BACKTRACE=1 RUST_LOG="ethercrab=trace,debug" ./target/release/examples/{{example}}
