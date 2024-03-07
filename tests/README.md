# Integration tests

Uses Wireshark captures of known-good runs as replays to test for regressions against.

## Capturing replays

Captures should be run in debug mode to make sure everything has time to breathe. If this is not
done, the replays can fail in weird and confusing ways.

### With `just`

From the repository root, run e.g.

```bash
just capture-replay replay-ek1100-el2828-el2889 enx00e04c680066
```

The replay name must **exactly** match the name of a file in `tests/` (without the `.rs`) extension.

Debian users may need to run:

- `sudo apt install psmisc`
- `cargo install fd-find`

Note that the Debian apt package `fd-find` installs the binary as `fd-find`, however the script
looks for just `fd`. Installing with `cargo` doesn't present this issue.

### Manually on local machine (terminal method)

`sudo apt install -y tshark`

Then e.g.

```bash
tshark  -w tests/replay-ek1100-el2828-el2889.pcapng --interface enp2s0 -f 'ether proto 0x88a4'
```

### Remote machine (GUI method)

On remote machine from project root e.g.

```bash
ssh ethercrab 'sudo tcpdump -U -i enp2s0 -w -' | wireshark -f 'ecat' -i - -w ./tests/replay-ek1100-el2828-el2889.pcapng -k
```

Then run the test using real interface e.g.:

```bash
INTERFACE=enp2s0 just linux-test replay
```

### Filtering captured replays

Use `tshark` to filter out only EtherCAT packets from provided dumps:

```bash
tshark -r EL1014.pcapng -Y 'ecat' -w issue-63-el1014.pcapng
```

This isn't required if the original capture is filtered with `-f 'ether proto 0x88a4'`.
