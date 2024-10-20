#![no_std]
#![no_main]
#![feature(type_alias_impl_trait, impl_trait_in_assoc_type)]

use embassy_executor::Spawner;

// #[embassy_executor::main]
// async fn main(spawner: Spawner) {
//     rusty_glove::main(spawner).await;
// }

use rusty_glove::{singleton, utils::MeasuringExecutor};

#[embassy_executor::task]
async fn asyncmain(spawner: Spawner) {
    rusty_glove::main(spawner).await;
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let executor = singleton!(MeasuringExecutor, MeasuringExecutor::new());
    // let executor = static_cell::make_static!(embassy_executor::Executor::new());

    executor.run(|spawner| {
        spawner.must_spawn(asyncmain(spawner));
    });
}
