left:
  env DEFMT_LOG="" cargo strip --release --bin binary --no-default-features --features side_left,default_unselected_side,reboot_on_panic -- --strip-all -o target/left.elf
  elf2uf2-rs --family-id 0x9807B007 target/left.elf left.uf2
right:
  env DEFMT_LOG="" cargo strip --release --bin binary --no-default-features --features side_right,default_unselected_side,reboot_on_panic -- --strip-all -o target/right.elf
  elf2uf2-rs --family-id 0x9808B007 target/right.elf right.uf2

flash:
  cargo strip --bin binary --release -- --strip-all -o target/binary.elf
  echo "Binary size is $(ls -lah target/binary.elf)"
  until picotool load -f ./target/binary.elf; do echo "trying again"; sleep 1; done
  picotool reboot

flash-bl:
  cargo build --release
  cp ./target/thumbv6m-none-eabi/release/boot ./target/boot.elf
  until picotool load -f ./target/boot.elf; do echo "trying again"; sleep 1; done

dbg-left:
  cargo objcopy --no-default-features --features probe -- target/binary.elf
  probe-rs-cli run --probe cafe:4005:6E16C4033956C9E2 --chip RP2040 target/binary.elf --speed 400
