# Integration tests

Uses Wireshark captures of known-good runs as replays to test for regressions against.

## Capturing replays

Captures should be run in debug mode to make sure everything has time to breathe. If this is not
done, the replays can fail in weird and confusing ways.

On remote machine from project root e.g.

```bash
ssh ethercrab 'sudo tcpdump -U -i enp2s0 -w -' | wireshark -f 'ecat' -i - -w ./tests/replay-ek1100-el2828-el2889.pcapng -k
```

Then run the test using real interface e.g.:

```bash
INTERFACE=enp2s0 just linux-test replay
```
