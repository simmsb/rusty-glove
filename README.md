# Rust firmware for the Glove80 keyboard

https://github.com/user-attachments/assets/92c6a0cc-923c-43d2-8932-4119070cfd70

Rusty glove is firmware for the Glove80 split keyboard written in Rust
using the Embassy framework.

## Layout

![Default Layout](layouts/rusty-glove.svg)

## Features

- Normal keypresses, mod taps, layers, chords, mouse keys
- Some pretty neopixel animations (that sync between sides, and transition smoothly)

## Wireless

The halves and a USB dongle share an IPv6 network, 6LoWPAN over IEEE 802.15.4,
and talk to each other over TCP. Addresses, ports and the radio settings live
in `shared/src/net.rs`.

| node   | address            | listens on                          |
|--------|--------------------|-------------------------------------|
| left   | `fd67:6c76:0:1::1` | 7300 (DFU)                          |
| right  | `fd67:6c76:0:1::2` | 7100 (the left half), 7300 (DFU)    |
| dongle | `fd67:6c76:0:1::3` | 7200 (keyboard reports)             |

The dongle is any nRF52840, flashed over SWD (`just dongle_flash`). Over USB
it's a keyboard, passing on the right half's reports, and an ethernet adapter.
It hands the host an address in `10.42.0.0/24` over DHCP (without a default
route), and forwards `10.42.0.1:7301` and `10.42.0.1:7302` to the left and
right halves' DFU ports. The network stack can't route, so those forwarded
ports are the only way through.

Update a half with `just left_net` or `just right_net`, which run
`tools/glove-dfu.py`. A half that boots into an update rolls it back on its
next reset unless the script reaches it again to confirm it.

## Building

Building is best done using the dev shell of the nix flake, it loads everything in that you need to build the project.

Then use `just both` to invoke the rules in the justfile for producing a `both.uf2` file.

## Keymaps

You can use https://github.com/simmsb/keylayout to generate key layouts (and
also generate previes of them), check out the [one I use](layouts/rusty-glove.kl)
