left:
  env DEFMT_LOG="" cargo strip --release --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic -- --strip-all -o target/left.elf
  elf2uf2-rs --family-id 0x9807B007 target/left.elf left.uf2

right:
  env DEFMT_LOG="" cargo strip --release --bin binary --no-default-features --features side_right,default_unselected_side,reboot_on_panic -- --strip-all -o target/right.elf
  elf2uf2-rs --family-id 0x9808B007 target/right.elf right.uf2

both: left right
  cat left.uf2 right.uf2 > both.uf2


left_dfu:
  env DEFMT_LOG="" cargo objcopy --release -p rusty-glove --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic -- -O ihex target/left.hex
  nrfutil pkg generate --debug-mode --application target/left.hex target/left.zip

left_flash: left_dfu
  nrfutil dfu ble -pkg target/left.zip -ic NRF52 -n "Glove80 LH" 

keymap:
  keylayout_lang emit -m rusty-dilemma layouts/rusty-glove.kl > firmware/src/keys/layout.rs
  keylayout_lang format -i layouts/rusty-glove.kl

draw:
  keylayout_lang emit -m keymap-drawer layouts/rusty-glove.kl > layouts/rusty-glove.yaml
  keymap draw layouts/rusty-glove.yaml > layouts/rusty-glove.svg
