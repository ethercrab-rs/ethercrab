# Integration tests

Uses Wireshark captures of known-good runs as replays to test for regressions against.

## Capturing replays

Captures should be run in debug mode to make sure everything has time to breath. If this is not
done, the replays can fail in weird and confusing ways.

On remote machine from project root e.g.

```bash
ssh ethercrab 'sudo tcpdump -U -i enp2s0 -w -' | wireshark -f 'ecat' -i - -w ./tests/replay-ek1100-el2828-el2889.pcapng -k
```

Then run the test using real interface e.g.:

```bash
cargo build --test
sudo INTERFACE=enp2s0 /home/james/Repositories/ethercrab/target/debug/deps/replay_ek1100_el2828_el2889-5cd69ca6114e6ce1
```
