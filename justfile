default:
  @just -l

on_kbd:
    glasgow run probe-rs -V 'A=SA' --swclk 'A3' --swdio 'A2'
on_dongle:
    glasgow run probe-rs -V 'B=SB' --swclk 'B0' --swdio 'B1'

off:
    glasgow safe

build:
  env DEFMT_LOG="off" cargo build -p rusty-glove --release --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic
  env DEFMT_LOG="off" cargo build -p rusty-glove --release --bin binary --no-default-features --features side_right,default_unselected_side,reboot_on_panic
  env DEFMT_LOG="off" cargo build -p dongle --release --bin dongle --no-default-features --features reboot_on_panic

left:
  env DEFMT_LOG="off" cargo strip -p rusty-glove --release --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic -- --strip-all -o target/left.elf

right:
  env DEFMT_LOG="off" cargo strip -p rusty-glove --release --bin binary --no-default-features --features side_right,default_unselected_side,reboot_on_panic -- --strip-all -o target/right.elf

both: left right

dongle:
  env DEFMT_LOG="off" cargo strip -p dongle --release --bin dongle --no-default-features --features reboot_on_panic -- --strip-all -o target/dongle.elf

# needs the bootloader on it already (`just bootloader` with the probe on the dongle)
dongle_flash: dongle
  probe-rs download --chip nRF52840_xxAA --protocol swd --speed 1600 target/dongle.elf
  probe-rs reset --chip nRF52840_xxAA --protocol swd --speed 1600

bootloader:
  cargo run -p bootloader --release --bin bootloader

# the application's logs won't show up while this bootloader is installed
bootloader_debug:
  env DEFMT_LOG=trace cargo run -p bootloader --release --bin bootloader --features logging

left_uf2: left
  elf2uf2-rs --family-id 0x9807B007 target/left.elf left.uf2
right_uf2: right
  elf2uf2-rs --family-id 0x9808B007 target/right.elf right.uf2

both_uf2: both
  cat left.uf2 right.uf2 > both.uf2

# flat binaries for updating over the network
left_bin:
  env DEFMT_LOG="off" cargo objcopy -p rusty-glove --release --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic -- -O binary target/left.bin

right_bin:
  env DEFMT_LOG="off" cargo objcopy -p rusty-glove --release --bin binary --no-default-features --features side_right,default_unselected_side,reboot_on_panic -- -O binary target/right.bin

dongle_bin:
  env DEFMT_LOG="off" cargo objcopy -p dongle --release --bin dongle --no-default-features --features reboot_on_panic -- -O binary target/dongle.bin

# update a half through the dongle, or the dongle itself
dongle_net: dongle_bin
  tools/glove-dfu.py dongle target/dongle.bin

left_net: left_bin
  tools/glove-dfu.py left target/left.bin

right_net: right_bin
  tools/glove-dfu.py right target/right.bin

# keep the image a half is running, e.g. after updating it over BLE
left_confirm:
  tools/glove-dfu.py left --confirm-only

right_confirm:
  tools/glove-dfu.py right --confirm-only

dongle_confirm:
  tools/glove-dfu.py dongle --confirm-only

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

DEFMT_LOG_DEF_TRACE := env("DEFMT", "trace,ekv=info,xarxa=info,embassy_net=info")

debug_left:
  env DEFMT_LOG={{DEFMT_LOG_DEF_TRACE}} cargo run -p rusty-glove --release --bin binary --no-default-features --features side_left,default_unselected_side

debug_right:
  env DEFMT_LOG={{DEFMT_LOG_DEF_TRACE}} cargo run -p rusty-glove --release --bin binary --no-default-features --features side_right,default_unselected_side

debug_dongle:
  env DEFMT_LOG={{DEFMT_LOG_DEF_TRACE}} cargo run -p dongle --release --bin dongle

attach_right:
  probe-rs attach --chip nRF52840_xxAA target/thumbv7em-none-eabihf/debug-trace/binary

TMPDIR := `mktemp -d`

BM_FLASH := "arm-none-eabi-gdb -nx --batch \
    -ex 'target extended-remote :2000' \
    -ex 'monitor swdp_scan' \
    -ex 'attach 1' \
    -ex 'load' \
    -ex 'compare-sections' \
    -ex 'kill'"

bm_debug BINARY:
  #!/bin/sh
  cat > {{TMPDIR}}/layout.kdl <<EOF
    layout {
      pane command="blackmagic"
      pane command="arm-none-eabi-gdb" {
        args "-nx" "-ex" "set pagination off" "-ex" "target extended-remote :2000" "-ex" "monitor swdp_scan" "-ex" "attach 1" "-ex" "load" "-ex" "compare-sections" "-ex" "kill" "-ex" "monitor swdp_scan" "-ex" "attach 1" "{{BINARY}}"
      }
      pane command="probe-rs" {
        args "attach" "--protocol" "swd" "--chip" "nRF52840_xxAA" "{{BINARY}}"
      } 
    }
  EOF
  zellij -n {{TMPDIR}}/layout.kdl

#  blackmagic &
#  {{BM_FLASH}} {{BINARY}}
#  probe-rs attach --protocol swd --chip nRF52840_xxAA {{BINARY}}

bm_left:
  env DEFMT_LOG={{DEFMT_LOG_DEF_TRACE}} cargo objcopy -p rusty-glove --release --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic -- target/left.elf
  just bm_debug target/left.elf
