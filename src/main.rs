use cpu_monitor::CpuInstant;
use log::{info, warn};
use openrgb::{data::Color, OpenRGB};
use ringbuffer::{AllocRingBuffer, RingBuffer};
use simple_logger::SimpleLogger;
use std::{error::Error, time::Duration};
use sysinfo::{MemoryRefreshKind, RefreshKind};
use tokio::net::TcpStream;
use tokio_retry::Retry;

const SAMPLE_TIME: f32 = 5.0; // seconds.
const SAMPLE_RATE: u64 = 500;
const SAMPLE_BUFFER_SIZE: usize = (SAMPLE_TIME * (1.0 + 1.0 / SAMPLE_RATE as f32)) as usize;

const WHITE_COLOR: Color = Color::new(127, 127, 127);
const RED_COLOR: Color = Color::new(127, 0, 0);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    SimpleLogger::new().env().init().unwrap();

    let client = connect_to_open_rgb_server().await?;
    info!(
        "Connected to OpenRGB server! Protocol version: {}",
        client.get_protocol_version()
    );

    let mut cpu_samples = AllocRingBuffer::new(SAMPLE_BUFFER_SIZE);

    let mut sys = sysinfo::System::new_with_specifics(
        RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()),
    );

    loop {
        // CPU utilization.
        let start = CpuInstant::now()?;
        tokio::time::sleep(Duration::from_millis(SAMPLE_RATE)).await;
        let end = CpuInstant::now()?;
        let duration = end - start;
        let cpu_usage = duration.non_idle() as f32;
        cpu_samples.push(cpu_usage);

        let cpu_usage = cpu_samples
            .iter()
            .copied()
            .reduce(|accum, sample| accum + sample)
            .unwrap_or_default();
        let cpu_usage = cpu_usage / cpu_samples.len() as f32;

        // Memory utilization.
        sys.refresh_memory();
        let memory_usage = sys.used_memory() as f32 / sys.total_memory() as f32;

        // Set the color.
        let controller_count = client.get_controller_count().await?;
        for controller_id in 0..controller_count {
            let controller = client.get_controller(controller_id).await?;
            let led_count = controller.leds.len();
            if led_count == 0 {
                warn!("Controller {} has no LEDs", controller.name);
                continue;
            }

            let colors: Vec<Color> = match controller.name.as_str() {
                "Corsair Dominator Platinum" => {
                    generate_gradient_led_colors(memory_usage, &WHITE_COLOR, &RED_COLOR, led_count)
                }
                "Corsair Commander Core" => {
                    let mut colors = Vec::with_capacity(led_count);

                    // Ring colors.
                    colors.extend(generate_gradient_led_colors(
                        cpu_usage,
                        &WHITE_COLOR,
                        &RED_COLOR,
                        24,
                    ));

                    // Ports (fans) colors.
                    for _ in 0..6 {
                        colors.extend(generate_block_led_colors(
                            cpu_usage,
                            &WHITE_COLOR,
                            &RED_COLOR,
                            5,
                        ));
                    }

                    colors
                }
                "G502 HERO Gaming Mouse" => {
                    generate_block_led_colors(cpu_usage, &WHITE_COLOR, &RED_COLOR, led_count)
                }
                "MSI X670E GAMING PLUS WIFI (MS-7E16)" => vec![], // Do nothing.

                _ => {
                    warn!("Unknown controller: {}", controller.name);
                    vec![]
                }
            };

            client.update_leds(controller_id, colors).await?;
        }

        tokio::task::yield_now().await;
    }
}

async fn connect_to_open_rgb_server() -> Result<OpenRGB<TcpStream>, Box<dyn Error>> {
    let retry_strategy = tokio_retry::strategy::FixedInterval::from_millis(5000);

    Retry::spawn(retry_strategy, || async {
        info!("Connecting to OpenRGB server...");

        OpenRGB::connect()
            .await
            .map_err(|e| Box::new(e) as Box<dyn Error>)
    })
    .await
}

fn lerp(value: f32, start: f32, end: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    start + value * (end - start)
}

fn lerp_color(value: f32, start_color: &Color, end_color: &Color) -> Color {
    Color::new(
        lerp(value, start_color.r as f32, end_color.r as f32).round() as u8,
        lerp(value, start_color.g as f32, end_color.g as f32).round() as u8,
        lerp(value, start_color.b as f32, end_color.b as f32).round() as u8,
    )
}

fn generate_gradient_led_colors(
    value: f32,
    start_color: &Color,
    end_color: &Color,
    size: usize,
) -> Vec<Color> {
    let scaled_value = value * size as f32;

    (0..size)
        .map(|index| {
            lerp_color(
                (scaled_value - index as f32).clamp(0.0, 1.0),
                start_color,
                end_color,
            )
        })
        .collect()
}

fn generate_block_led_colors(
    value: f32,
    start_color: &Color,
    end_color: &Color,
    size: usize,
) -> Vec<Color> {
    vec![lerp_color(value, start_color, end_color); size]
}
