//! Configure a Omron R88d series drive and turn the motor.

use ethercrab::{
    ds402::{Ds402, Ds402Sm},
    error::Error,
    std::tx_rx_task,
    Client, ClientConfig, PduStorage, SlaveGroup, SlaveState, Timeouts, Field,
    sdo::*,
};
use futures_lite::future;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::time::MissedTickBehavior;



#[repr(u8)]
pub enum OperationMode {
	Off = 0,
	ProfilePosition = 1,
	Velocity = 2,
	ProfileVelocity = 3,
	TorqueProfile = 4,
	Homing = 6,
	InterpolatedPosition = 7,
	
	SynchronousPosition = 8,
	SynchronousVelocity = 9,
	SynchronousTorque = 10,
	SynchronousTorqueCommutation = 11,
}


/// Maximum number of slaves that can be stored.
const MAX_SLAVES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 1100;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 128;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();


struct Profile {
	mode: SubItem<u8>,
	period: ProfilePeriod,
	current: ProfileCurrent,
	target: ProfileTarget,
	pdo: [ConfigurablePdo; 2],
	rx: SyncManager,
	tx: SyncManager,
	gear: ProfileGear,
}

struct ProfilePeriod {
	digits: SubItem<u8>,
	exponent: SubItem<i8>,
}
struct ProfileCurrent {
	status: SubItem<u16>,
	error: SubItem<u16>,
	position: SubItem<i32>,
	velocity: SubItem<i32>,
	force: SubItem<i16>,
}
struct ProfileTarget {
	control: SubItem<u16>,
	position: SubItem<i32>,
	velocity: SubItem<i32>,
	force: SubItem<i16>,
}
struct ProfileGear {
	motor: SubItem<u32>,
	shaft: SubItem<u32>,
}

#[derive(Clone, Debug)]
struct Vars {
	error: Field<u16>,
	status: Field<u16>,
	control: Field<u16>,
	current: Field<i32>,
	target: Field<i32>,
}


#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::init();

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
		}).expect("Error setting Ctrl-C handler");
		
	future::race(
		async { 
			loop {
				tokio::time::sleep(Duration::from_secs_f32(1.)).await;
				if ! running.load(Ordering::Relaxed)  {break}
			}
			Ok(())
		},
		async {
			let profile = Arc::new(Profile {
				mode: SubItem::<u8> {index: 0x6060, sub: 0, field: Field::new(0, 0, 8)},
				current: ProfileCurrent {
					status: SubItem::<u16> {index: 0x6041, sub: 0, field: Field::new(0, 0, 16)},
					error: SubItem::<u16> {index: 0x603f, sub: 0, field: Field::new(0, 0, 16)},
					position: SubItem::<i32> {index: 0x6064, sub: 0, field: Field::new(0, 0, 32)},
					velocity: SubItem::<i32> {index: 0x606c, sub: 0, field: Field::new(0, 0, 32)},
					force: SubItem::<i16> {index: 0x6077, sub: 0, field: Field::new(0, 0, 16)},
					},
				target: ProfileTarget {
					control: SubItem::<u16> {index: 0x6040, sub: 0, field: Field::new(0, 0, 16)},
					position: SubItem::<i32> {index: 0x607a, sub: 0, field: Field::new(0, 0, 32)},
					velocity: SubItem::<i32> {index: 0x60ff, sub: 0, field: Field::new(0, 0, 32)},
					force: SubItem::<i16> {index: 0x6071, sub: 0, field: Field::new(0, 0, 32)},
					},
				period: ProfilePeriod {
					digits: SubItem::<u8> {index: 0x60c2, sub: 1, field: Field::new(2, 0, 8)},
					exponent: SubItem::<i8> {index: 0x60c2, sub: 2, field: Field::new(3, 0, 8)},
					},
				pdo: [
					ConfigurablePdo {index: 0x1600, num: 3},
					ConfigurablePdo {index: 0x1a00, num: 3},
					],
				rx: SyncManager {index: 0x1c12, num: 3},
				tx: SyncManager {index: 0x1c13, num: 3},
				gear: ProfileGear {
					motor: SubItem {index: 0x6091, sub: 1, field: Field::new(2, 0, 32)},
					shaft: SubItem {index: 0x6091, sub: 2, field: Field::new(6, 0, 32)},
					},
				});
			
			let interface = std::env::args()
				.nth(1)
			.expect("Provide interface as first argument. Pass an unrecognised name to list available interfaces.");

			log::info!("Starting SDO demo...");

			let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
			let client = Arc::new(Client::new(
				pdu_loop,
				Timeouts {
					wait_loop_delay: Duration::from_millis(2),
					mailbox_response: Duration::from_millis(1000),
					..Default::default()
				},
				ClientConfig::default(),
			));
			tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

			use tokio::sync::Mutex;
			let vars = Arc::new(Mutex::new(None)) as Arc<Mutex<Option<Vars>>>;

			let groups = {
				let profile = profile.clone();
				let vars = vars.clone();
				SlaveGroup::<MAX_SLAVES, PDI_LEN>::new(Box::new(move |slave| {
					let profile = profile.clone();
					let vars = vars.clone();
					Box::pin(async move {
						let start = Instant::now();
						
						profile.gear.motor.set(slave, 1).await?;
						profile.gear.shaft.set(slave, 1).await?;
						log::debug!("gearbox: {} / {}", 
							profile.gear.shaft.get(slave).await?,
							profile.gear.motor.get(slave).await?,
							);
						
						let mut sm = SyncMapping::new(slave, &profile.rx).await?;
							let mut pdo = sm.push(&profile.pdo[0]).await?;
								let vars_control = pdo.push(&profile.target.control).await?;
								let vars_target = pdo.push(&profile.target.position).await?;
								pdo.finish().await?;
							sm.finish().await?;
						
						let mut sm = SyncMapping::new(slave, &profile.tx).await?;
							let mut pdo = sm.push(&profile.pdo[1]).await?;
								let vars_status = pdo.push(&profile.current.status).await?;
								let vars_current = pdo.push(&profile.current.position).await?;
								let vars_error = pdo.push(&profile.current.error).await?;
								pdo.finish().await?;
							sm.finish().await?;

						*vars.lock().await = Some(Vars {
							error: vars_error,
							status: vars_status,
							control: vars_control,
							current: vars_current,
							target: vars_target,
							});
						profile.mode.set(slave, OperationMode::SynchronousPosition as u8).await?;

						let stop = Instant::now();
						log::info!("configured {} in {}", slave.name(), stop.duration_since(start).as_secs_f32());
						Ok(())
					})
				}))
			};

			let group = client
				.init::<16, _>(groups, |groups, _slave| Ok(groups.as_mut()))
				.await.expect("Init");
			client
				.request_slave_state(SlaveState::Op)
				.await.expect("OP");
			let vars = vars.lock().await.clone().unwrap();
			
			log::info!("offsets {:#?}", vars);

			log::info!("Slaves moved to OP state");
			log::info!("Group has {} slaves", group.len());
			for slave in group.slaves() {
				let (i, o) = slave.io();

				log::info!(
					"-> Slave {} {} inputs: {} bytes, outputs: {} bytes",
					slave.configured_address,
					slave.name,
					i.len(),
					o.len(),
				);
			}

			// Run twice to prime PDI
			group.tx_rx(&client).await.expect("TX/RX");

			let cycle_time = {
				let slave = group.slave(0).unwrap().bind(&client);

				let base = f32::from(profile.period.digits.get(&slave).await?);
				let exponent = i32::from(profile.period.exponent.get(&slave).await?);

				Duration::from_secs_f32(base * 10f32.powi(exponent))
			};

			log::info!("Cycle time: {} ms", cycle_time.as_millis());

			let mut cyclic_interval = tokio::time::interval(cycle_time);
			cyclic_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

			let slave = group.slave(0).expect("No servo!");
			let mut servo = Ds402Sm::new(Ds402::new(slave).expect("Failed to gather DS402"));
			
			let initial = {
				group.tx_rx(&client).await.expect("TX/RX");
				let (i, _o) = servo.slave().io();
				vars.current.get(i)
				};
			let velocity = 3_000_000;
			
			log::info!("going forward");

			loop {
				cyclic_interval.tick().await;
				group.tx_rx(&client).await.expect("TX/RX");
				
				if servo.tick() {
					let (i, o) = servo.slave().io();

					let status = vars.status.get(i);
					let error = vars.error.get(i);
					let pos = vars.current.get(i);

					log::debug!("Position: {pos}, error: {error:x}, fault: {:x}", status);
					log::debug!("{:?}", status);
					if pos > initial+800_000_000 {break}
					
					vars.target.set(o, pos+velocity);
				}
			}

			log::info!("going backward");

			loop {
				cyclic_interval.tick().await;
				group.tx_rx(&client).await.expect("TX/RX");
				
				if servo.tick() {
					let (i, o) = servo.slave().io();

					let status = vars.status.get(i);
					let error = vars.error.get(i);
					let pos = vars.current.get(i);

					log::debug!("Position: {pos}, error: {error:x}, fault: {:x}", status);
					if pos < initial-100 {break}
					
					vars.target.set(o, pos-velocity);
				}
			}
			
			Ok(())
		}).await

}
