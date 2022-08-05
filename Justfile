linux-example example:
     cargo build --example {{example}} && sudo setcap cap_net_raw=pe ./target/debug/examples/{{example}} && RUST_BACKTRACE=1 RUST_LOG="ethercrab=trace,debug" ./target/debug/examples/{{example}}

linux-example-release example:
     cargo build --example {{example}} --release && sudo setcap cap_net_raw=pe ./target/release/examples/{{example}} && RUST_BACKTRACE=1 RUST_LOG="ethercrab=trace,debug" ./target/release/examples/{{example}}
