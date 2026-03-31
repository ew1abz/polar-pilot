use embassy_net::Stack;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::Output;
use embassy_stm32::spi::{self, Spi};
use embassy_time::{Duration, Ticker, with_timeout};
use embedded_hal_bus::spi::ExclusiveDevice;

// Static fallback assigned when DHCP does not reply within DHCP_TIMEOUT.
// Adjust to match your local network.
pub const STATIC_IP: [u8; 4]      = [192, 168, 1, 200];
pub const STATIC_GW: [u8; 4]      = [192, 168, 1, 1];
pub const STATIC_PREFIX_LEN: u8   = 24;
const     DHCP_TIMEOUT: Duration  = Duration::from_secs(120);

#[embassy_executor::task]
pub async fn led_task(mut led: Output<'static>) -> ! {
    let mut ticker = Ticker::every(Duration::from_secs(1));
    loop {
        ticker.next().await;
        led.toggle();
    }
}

#[embassy_executor::task]
pub async fn ethernet_task(
    runner: embassy_net_wiznet::Runner<
        'static,
        embassy_net_wiznet::chip::W5500,
        ExclusiveDevice<
            Spi<'static, embassy_stm32::mode::Async, spi::mode::Master>,
            Output<'static>,
            embassy_time::Delay,
        >,
        ExtiInput<'static, embassy_stm32::mode::Async>,
        Output<'static>,
    >,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
pub async fn net_task(
    mut runner: embassy_net::Runner<'static, embassy_net_wiznet::Device<'static>>,
) -> ! {
    runner.run().await
}

/// Waits up to DHCP_TIMEOUT for an IP address.  If none arrives, falls back
/// to the predefined static address so the rotator is always reachable.
#[embassy_executor::task]
pub async fn dhcp_watchdog_task(stack: Stack<'static>) -> ! {
    match with_timeout(DHCP_TIMEOUT, stack.wait_config_up()).await {
        Ok(_) => {
            defmt::info!("net: DHCP acquired");
        }
        Err(_) => {
            defmt::warn!("net: DHCP timeout — falling back to static {}.{}.{}.{}",
                STATIC_IP[0], STATIC_IP[1], STATIC_IP[2], STATIC_IP[3]);
            stack.set_config_v4(embassy_net::ConfigV4::Static(embassy_net::StaticConfigV4 {
                address: embassy_net::Ipv4Cidr::new(
                    embassy_net::Ipv4Address::new(
                        STATIC_IP[0], STATIC_IP[1], STATIC_IP[2], STATIC_IP[3],
                    ),
                    STATIC_PREFIX_LEN,
                ),
                gateway: Some(embassy_net::Ipv4Address::new(
                    STATIC_GW[0], STATIC_GW[1], STATIC_GW[2], STATIC_GW[3],
                )),
                dns_servers: Default::default(),
            }));
        }
    }

    loop { embassy_time::Timer::after_secs(3600).await; }
}
