# Unix datagram proof carrier

This standalone host-only crate is a concrete `Transport` witness. It is not a
Hibana public API, a Pico dependency, or a recommended production carrier.

The connected Unix datagram pair supplies one fresh, peer-bound carrier
generation. The adapter preserves Hibana's exact eight-byte observation header,
publishes data atomically, drains data before a same-direction close marker,
rejects malformed or wrong-peer packets, consumes each datagram once, wakes
receivers on logical close, and keeps requeue local to the unresolved receipt.

The proof boundary is deliberately explicit:

- the operating system supplies reliable ordered datagrams for one connected
  Unix socket pair;
- the Rust conformance tests connect two independent Hibana runtimes and check
  FIFO delivery, close wakeup, and generation isolation;
- Lean's carrier hierarchy proves the protocol consequences of the corresponding
  `Closing` assumptions;
- process crash detection, scheduler fairness, authentication beyond the socket
  pair, and arbitrary third-party `Transport` implementations remain deployment
  premises.

No protocol image, epoch, carrier profile, or proof metadata enters a Hibana
wire frame or endpoint.
