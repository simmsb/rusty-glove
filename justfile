default:
  @just -l

build:
  env DEFMT_LOG="off" cargo build -p rusty-glove --release --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic
  env DEFMT_LOG="off" cargo build -p rusty-glove --release --bin binary --no-default-features --features side_right,default_unselected_side,reboot_on_panic

left:
  env DEFMT_LOG="off" cargo strip -p rusty-glove --release --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic -- --strip-all -o target/left.elf

right:
  env DEFMT_LOG="off" cargo strip -p rusty-glove --release --bin binary --no-default-features --features side_right,default_unselected_side,reboot_on_panic -- --strip-all -o target/right.elf

both: left right

bootloader:
  env DEFMT_LOG=trace cargo run -p bootloader --release --bin bootloader --features turbowakers

left_uf2: left
  elf2uf2-rs --family-id 0x9807B007 target/left.elf left.uf2
right_uf2: right
  elf2uf2-rs --family-id 0x9808B007 target/right.elf right.uf2

both_uf2: both
  cat left.uf2 right.uf2 > both.uf2

info:
  ble-dfu info

left_ble: left
  ble-dfu update left target/left.elf

right_ble: right
  ble-dfu update right target/right.elf

# left_dfu:
#   env DEFMT_LOG="" cargo objcopy --release -p rusty-glove --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic -- -O ihex target/left.hex
#   nrfutil pkg generate --debug-mode --application target/left.hex target/left.zip

# left_flash: left_dfu
#   nrfutil dfu ble -pkg target/left.zip -ic NRF52 -n "Glove80 LH"

keymap:
  keylayout_lang emit -m rusty-dilemma layouts/rusty-glove.kl > firmware/src/keys/layout.rs
  keylayout_lang format -i layouts/rusty-glove.kl

draw:
  keylayout_lang emit -m keymap-drawer layouts/rusty-glove.kl > layouts/rusty-glove.yaml
  keymap draw layouts/rusty-glove.yaml > layouts/rusty-glove.svg

DEFMT_LOG_DEF_TRACE := "trace,ekv=info"

debug_left:
  env DEFMT_LOG={{DEFMT_LOG_DEF_TRACE}} cargo run -p rusty-glove --profile debug-trace --bin binary --no-default-features --features side_left,default_unselected_side

debug_right:
  env DEFMT_LOG={{DEFMT_LOG_DEF_TRACE}} cargo run -p rusty-glove --profile debug-trace --bin binary --no-default-features --features side_right,default_unselected_side

attach_right:
  probe-rs attach --chip nRF52840_xxAA target/thumbv7em-none-eabihf/debug-trace/binary
