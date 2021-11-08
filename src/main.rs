use minifb::{Key, KeyRepeat, Window, WindowOptions};
use plotters::prelude::*;
use plotters_bitmap::bitmap_pixel::BGRXPixel;
use plotters_bitmap::BitMapBackend;
use rumqttc::{v4::Packet, Client, Event, MqttOptions, QoS};
use std::borrow::{Borrow, BorrowMut};
use std::error::Error;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};
const W: usize = 1600;
const H: usize = 800;

// const SAMPLE_RATE: f64 = 200.0;
// const FREAME_RATE: f64 = 30.0;

const DATA_LENGTH: usize = 1000;

struct BufferWrapper(Vec<u32>);
impl Borrow<[u8]> for BufferWrapper {
    fn borrow(&self) -> &[u8] {
        // Safe for alignment: align_of(u8) <= align_of(u32)
        // Safe for cast: u32 can be thought of as being transparent over [u8; 4]
        unsafe { std::slice::from_raw_parts(self.0.as_ptr() as *const u8, self.0.len() * 4) }
    }
}
impl BorrowMut<[u8]> for BufferWrapper {
    fn borrow_mut(&mut self) -> &mut [u8] {
        // Safe for alignment: align_of(u8) <= align_of(u32)
        // Safe for cast: u32 can be thought of as being transparent over [u8; 4]
        unsafe { std::slice::from_raw_parts_mut(self.0.as_mut_ptr() as *mut u8, self.0.len() * 4) }
    }
}
impl Borrow<[u32]> for BufferWrapper {
    fn borrow(&self) -> &[u32] {
        self.0.as_slice()
    }
}
impl BorrowMut<[u32]> for BufferWrapper {
    fn borrow_mut(&mut self) -> &mut [u32] {
        self.0.as_mut_slice()
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut buf = BufferWrapper(vec![0u32; W * H]);

    let mut window = Window::new(
        // &get_window_title(fx, fy, yphase - xphase),
        "Pressure Data         s=Save    <Esc>=Exit",
        W,
        H,
        WindowOptions::default(),
    )?;
    let root =
        BitMapBackend::<BGRXPixel>::with_buffer_and_format(buf.borrow_mut(), (W as u32, H as u32))?
            .into_drawing_area();
    root.fill(&BLACK)?;

    let mut chart = ChartBuilder::on(&root)
        .margin(10)
        .set_all_label_area_size(50)
        .build_cartesian_2d(0.0..120.0, -50_000.0..2_500.0)?;

    chart
        .configure_mesh()
        .label_style(("sans-serif", 15).into_font().color(&GREEN))
        .axis_style(&GREEN)
        .draw()?;

    let cs = chart.into_chart_state();
    drop(root);

    let mut data: Vec<(SystemTime, f64)> = Vec::new();

    let mut mqttoptions = MqttOptions::new("pressure_data_receiver", "raspberrypi.local", 1883);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    let (mut client, mut connection) = Client::new(mqttoptions, 10);
    client
        .subscribe("pressure/data", QoS::AtMostOnce)
        .expect("Mqtt subscribe failed");

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for notification in connection.iter() {
            // debug:
            // println!("notification: {:?}", notification);

            // get pressure data
            if let Ok(event) = notification {
                match event {
                    Event::Incoming(Packet::Publish(publish)) => {
                        let bytes = publish.payload;
                        if bytes.len() == 4 {
                            let pressure =
                                f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64;
                            tx.send(pressure).ok();
                        }
                    }
                    _ => {
                        continue;
                    }
                }
            };
        }
    });

    let mut start_ts = SystemTime::now();
    // let mut last_flushed = 0.0;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if let Ok(pressure) = rx.recv() {
            // debug:
            println!("Pressure: {}", pressure);

            let now = SystemTime::now();

            if data.len() == 0 {
                start_ts = now;
            }

            if data.len() > DATA_LENGTH {
                data.remove(0);
                start_ts = data[0].0;
            }

            data.push((now, pressure));

            let root = BitMapBackend::<BGRXPixel>::with_buffer_and_format(
                buf.borrow_mut(),
                (W as u32, H as u32),
            )?
            .into_drawing_area();
            let mut chart = cs.clone().restore(&root);
            chart.plotting_area().fill(&BLACK)?;

            chart
                .configure_mesh()
                .bold_line_style(&GREEN.mix(0.2))
                .light_line_style(&TRANSPARENT)
                .draw()?;

            let chart_data: Vec<(f64, f64)> = data
                .iter()
                .map(|d| {
                    (
                        d.0.duration_since(start_ts)
                            .expect("Duration calculate failed")
                            .as_secs_f64(),
                        d.1,
                    )
                })
                .collect();

            chart.draw_series(chart_data.iter().zip(chart_data.iter().skip(1)).map(
                |(&(t0, p0), &(t1, p1))| PathElement::new(vec![(t0, p0), (t1, p1)], &GREEN),
            ))?;

            drop(root);
            drop(chart);

            if let Some(keys) = window.get_keys_pressed(KeyRepeat::Yes) {
                for key in keys {
                    match key {
                        Key::S => {
                            let mut wtr = csv::Writer::from_path("pressure_data.csv")?;
                            wtr.write_record(&["Time(s)", "Pressure(Pa)"])?;

                            for data in &chart_data {
                                wtr.write_record(&[data.0.to_string(), data.1.to_string()])?;
                            }

                            wtr.flush()?;
                            continue;
                        }
                        _ => {
                            continue;
                        }
                    }
                }
            }

            window.update_with_buffer(buf.borrow(), W, H)?;
        }
    }
    Ok(())
}
