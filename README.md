# osc-booper

osc-booper is an [OSC](https://en.wikipedia.org/wiki/Open_Sound_Control)-based CLI tool that listens for messages on the
channel `/OSCBoop` (configurable) and sends boop statistics
to the [VRChat chatbox](https://docs.vrchat.com/docs/osc-as-input-controller).

For a ready-to-use unity package & software, check out ValueFactory's [Boop Counter](https://boop.shader.gay).

# Installation

```bash
cargo install --git https://github.com/Invisi/osc-booper.git
```

# Usage

To run it, use one of the following commands:

```bash
# with default port (send on 9000)
osc-booper

# with custom port
osc-booper --send 9000

# create config.toml, allows persisting custom port
osc-booper --send 9000 --save
```

For more details, check the help via `osc-booper --help`.

The listen port will be auto-negotiated with VRChat via OSCQuery & mDNS service discovery.
