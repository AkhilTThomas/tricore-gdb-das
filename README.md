# tricore-gdb-das
Provide a GDB experience for AURIX controllers using a miniwiggler debugger.

This repository uses the MCD interfaces on rust (rust-mcd) from the [veecle-tricore-probe](https://github.com/veecle/tricore-probe) and the interface files to access them.

## Feature
- Flashing
- SW breakpoints
- Single Step
- Continue
- multicore support

## Pending Features
- backtrace depth > 2
- upload
- reset run

## Pre-requisistes
- [tricore-gdb](https://github.com/NoMore201/tricore-gdb.git)

## Getting started

```
cargo run -- --tcp_ip <ip>
```
Launch gdb from either vscode or gdb cmdline. A reference launch config is available [here](docs/launch.json)

Refer <https://github.com/AkhilTThomas/tc397_tft> for sample usage
