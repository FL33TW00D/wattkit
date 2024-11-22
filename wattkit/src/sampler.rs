use core_foundation::dictionary::CFDictionaryRef;
use oneshot::channel as oneshot_channel;
use oneshot::Sender as OneshotSender;
use std::{
    sync::mpsc::{channel, Receiver},
    thread::JoinHandle,
};

use crate::cf_utils::from_cfstr;
use crate::cf_utils::get_cf_string;
use crate::io_report::CVoidRef;
use crate::io_report::IOReportArrayGetValueAtIndex;
use crate::io_report::IOReportChannelRequest;
use crate::io_report::IOReportSampleCopyDescription;
use crate::io_report::IOReportSimpleGetIntegerValue;
use crate::io_report::IOReportStateGetCount;
use crate::io_report::IOReportStateGetNameForIndex;
use crate::io_report::IOReportStateGetResidency;
use crate::io_report::{EnergyUnit, IOReport, IOReportChannelGroup, IOReportChannelName};

#[derive(thiserror::Error, Debug)]
pub enum SamplerError {
    #[error("IOReportError: {0}")]
    IOReportError(#[from] crate::io_report::IOReportError),
    #[error("No samples available")]
    SamplesNotAvailable,
    #[error("Sampling in progress")]
    SamplingInProgress,
    #[error("No sampling currently in progress")]
    NoSamplingInProgress,
}

#[derive(Clone, Debug, Default)]
pub struct EnergySample {
    cpu_energy: u128,
    gpu_energy: u128,
    ane_energy: u128,
    duration: u64, //milliseconds
}

#[derive(Debug)]
struct SampleManager {
    cancel_sender: OneshotSender<()>,
    sample_receiver: Receiver<EnergySample>,
    thread_handle: JoinHandle<()>,
}

pub fn cfio_get_residencies(item: CFDictionaryRef) -> Vec<(String, i64)> {
    let count = unsafe { IOReportStateGetCount(item) };
    let mut res = vec![];

    for i in 0..count {
        let name = unsafe { IOReportStateGetNameForIndex(item, i) };
        let val = unsafe { IOReportStateGetResidency(item, i) };
        res.push((from_cfstr(name), val));
    }

    res
}

impl SampleManager {
    fn new(duration: u64, num_samples: usize) -> Self {
        let (cancel_tx, cancel_rx) = oneshot_channel();
        let (sample_tx, sample_rx) = channel();

        let handle = std::thread::spawn(move || {
            //let requests = vec![IOReportChannelRequest::new(
            //    IOReportChannelGroup::EnergyModel,
            //    None as Option<IOReportChannelName>,
            //)];
            let mut report = IOReport::new(vec![]).unwrap();

            loop {
                if cancel_rx.try_recv().is_ok() {
                    break;
                }

                let samples = report.get_samples(duration, num_samples);
                for mut sample in samples {
                    let duration = sample.duration();
                    let mut energy_sample = EnergySample {
                        duration,
                        ..Default::default()
                    };

                    for entry in sample.iterator_mut() {
                        match entry.group {
                            IOReportChannelGroup::EnergyModel => {
                                let u = EnergyUnit::from(entry.unit);
                                let raw_joules = unsafe {
                                    IOReportSimpleGetIntegerValue(entry.item, std::ptr::null_mut())
                                } as u128;
                                let milli_joules = match u {
                                    EnergyUnit::NanoJoules => raw_joules / 1_000_000,
                                    EnergyUnit::MicroJoules => raw_joules / 1_000,
                                    EnergyUnit::MilliJoules => raw_joules,
                                };

                                match entry.channel_name {
                                    IOReportChannelName::CPUEnergy => {
                                        energy_sample.cpu_energy += milli_joules
                                    }
                                    IOReportChannelName::GPUEnergy => {
                                        energy_sample.gpu_energy += milli_joules
                                    }
                                    IOReportChannelName::ANE => {
                                        energy_sample.ane_energy += milli_joules
                                    }
                                    _ => {}
                                };
                            }
                            IOReportChannelGroup::H11ANE => match entry.channel_name {
                                IOReportChannelName::Unknown(ref u)
                                    if u == "Allocated Shared Memory" =>
                                {
                                    println!("{:?}", entry);

                                    let cv = entry.item as CVoidRef;
                                    println!("{:?}", cv);

                                    let desc = get_cf_string(|| unsafe {
                                        IOReportSampleCopyDescription(entry.item, 0)
                                    });
                                    println!("Desc: {:?}", desc);
                                }
                                _ => {}
                            },
                            _ => continue,
                        }
                    }
                    if sample_tx.send(energy_sample).is_err() {
                        break;
                    }
                }
            }
        });

        SampleManager {
            cancel_sender: cancel_tx,
            sample_receiver: sample_rx,
            thread_handle: handle,
        }
    }

    fn stop(self) -> Vec<EnergySample> {
        let _ = self.cancel_sender.send(());
        let mut samples = Vec::with_capacity(128);
        while let Ok(sample) = self.sample_receiver.recv() {
            samples.push(sample);
        }
        let _ = self.thread_handle.join();
        samples
    }
}

pub trait Sampling {
    fn samples(&self) -> Option<&Vec<EnergySample>>;

    fn start_time(&self) -> Option<std::time::Instant>;

    fn end_time(&self) -> Option<std::time::Instant>;

    fn profile(&self) -> Result<PowerProfile, SamplerError> {
        if let Some(samples) = self.samples() {
            Ok(PowerProfile::from(samples))
        } else {
            Err(SamplerError::SamplesNotAvailable)
        }
    }

    fn duration(&self) -> Option<u64> {
        if let (Some(start), Some(end)) = (self.start_time(), self.end_time()) {
            Some(end.duration_since(start).as_secs())
        } else {
            None
        }
    }
}

/// # Sampler
///
/// The `Sampler` struct is used to sample the power consumption of the device.
/// When sampling begins, the `Sampler` will `subscribe` to the underlying IOReport
/// C API, and will begin to receive power samples.
///
/// These values are placed onto a queue, which can then be accessed by the user.
///
/// ## Example
/// ```rust
/// use wattkit::*;
///
/// let sampler = Sampler::new();
/// {
///     // Start sampling
///     let guard = sampler.subscribe(1000); //sample every 1000ms
///
///     // Do some work
///     for x in 0..1000000 {
///         let y = x * x;
///     }
/// }
/// let profile = sampler.power_profile();
#[derive(Debug, Default)]
pub struct GuardSampler {
    start_time: Option<std::time::Instant>,
    end_time: Option<std::time::Instant>,
    samples: Option<Vec<EnergySample>>,
}

pub struct SamplerGuard<'a> {
    sampler: &'a mut GuardSampler,
    manager: Option<SampleManager>,
}

impl<'a> Drop for SamplerGuard<'a> {
    fn drop(&mut self) {
        if let Some(manager) = self.manager.take() {
            self.sampler.end_time = Some(std::time::Instant::now());
            self.sampler.samples = Some(manager.stop());
        }
    }
}

impl GuardSampler {
    pub fn new() -> Self {
        GuardSampler::default()
    }

    pub fn subscribe(&mut self, duration: u64, num_samples: usize) -> SamplerGuard {
        self.start_time = Some(std::time::Instant::now());
        SamplerGuard {
            sampler: self,
            manager: Some(SampleManager::new(duration, num_samples)),
        }
    }
}

impl Sampling for GuardSampler {
    fn samples(&self) -> Option<&Vec<EnergySample>> {
        self.samples.as_ref()
    }

    fn start_time(&self) -> Option<std::time::Instant> {
        self.start_time
    }

    fn end_time(&self) -> Option<std::time::Instant> {
        self.end_time
    }
}

/// # StartStopSampler
///
/// Exclusively for use with pyo3, use `Sampler` from Rust instead.
#[derive(Debug, Default)]
pub struct StartStopSampler {
    samples: Option<Vec<EnergySample>>,
    manager: Option<SampleManager>,
    start_time: Option<std::time::Instant>,
    end_time: Option<std::time::Instant>,
}

impl StartStopSampler {
    pub fn new() -> Self {
        StartStopSampler::default()
    }

    pub fn start(&mut self, duration: u64, num_samples: usize) -> Result<(), &'static str> {
        if self.manager.is_some() {
            return Err("Sampling is already in progress");
        }
        self.start_time = Some(std::time::Instant::now());
        self.manager = Some(SampleManager::new(duration, num_samples));
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), &'static str> {
        if let Some(core) = self.manager.take() {
            self.end_time = Some(std::time::Instant::now());
            self.samples = Some(core.stop());
            Ok(())
        } else {
            Err("No sampling in progress")
        }
    }

    pub fn is_sampling(&self) -> bool {
        self.manager.is_some()
    }
}

impl Sampling for StartStopSampler {
    fn samples(&self) -> Option<&Vec<EnergySample>> {
        self.samples.as_ref()
    }

    fn start_time(&self) -> Option<std::time::Instant> {
        self.start_time
    }

    fn end_time(&self) -> Option<std::time::Instant> {
        self.end_time
    }
}

#[derive(Debug, Default)]
pub struct PowerProfile {
    total_cpu_energy: u128,
    total_gpu_energy: u128,
    total_ane_energy: u128,
    total_cpu_milliwatts: u64,
    total_gpu_milliwatts: u64,
    total_ane_milliwatts: u64,
    total_energy: u128,
    total_power: u64,
    total_duration: u64,
}

impl<C: AsRef<[EnergySample]>> From<C> for PowerProfile {
    fn from(samples: C) -> Self {
        let mut profile = PowerProfile::default();
        let samples = samples.as_ref();

        let mut total_cpu_milliwatts = 0.;
        let mut total_gpu_milliwatts = 0.;
        let mut total_ane_milliwatts = 0.;
        for s in samples.iter() {
            let duration_secs = s.duration as f64 / 1000.0; //mJs-1 == mW

            profile.total_cpu_energy += s.cpu_energy;
            profile.total_gpu_energy += s.gpu_energy;
            profile.total_ane_energy += s.ane_energy;
            total_cpu_milliwatts += s.cpu_energy as f64 / duration_secs;
            total_gpu_milliwatts += s.gpu_energy as f64 / duration_secs;
            total_ane_milliwatts += s.ane_energy as f64 / duration_secs;
            profile.total_duration += s.duration;
        }

        let num_samples = samples.len() as f64;
        profile.total_cpu_milliwatts = f64::round(total_cpu_milliwatts / num_samples) as u64;
        profile.total_gpu_milliwatts = f64::round(total_gpu_milliwatts / num_samples) as u64;
        profile.total_ane_milliwatts = f64::round(total_ane_milliwatts / num_samples) as u64;

        profile.total_energy =
            profile.total_cpu_energy + profile.total_gpu_energy + profile.total_ane_energy;
        profile.total_power = profile.total_cpu_milliwatts
            + profile.total_gpu_milliwatts
            + profile.total_ane_milliwatts;

        profile
    }
}

impl std::fmt::Display for PowerProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Total Energy: {} mJ\nTotal Power: {} mW\nTotal Duration: {} ms\nCPU Energy: {} mJ\nGPU Energy: {} mJ\nANE Energy: {} mJ\nCPU Power: {} mW\nGPU Power: {} mW\nANE Power: {} mW",
            self.total_energy,
            self.total_power,
            self.total_duration,
            self.total_cpu_energy,
            self.total_gpu_energy,
            self.total_ane_energy,
            self.total_cpu_milliwatts,
            self.total_gpu_milliwatts,
            self.total_ane_milliwatts
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guard_sampler() {
        let mut sampler = GuardSampler::new();
        {
            let _guard = sampler.subscribe(100, 2);
            std::thread::sleep(std::time::Duration::from_secs(8))
        }
        let profile = sampler.profile().unwrap();
        println!("{}", profile);
    }

    #[test]
    fn test_start_stop_sampler() {
        let mut sampler = StartStopSampler::new();

        assert!(!sampler.is_sampling());
        sampler.start(100, 2).unwrap();
        assert!(sampler.is_sampling());

        std::thread::sleep(std::time::Duration::from_secs(5));

        sampler.stop().unwrap();
        assert!(!sampler.is_sampling());
        let profile = sampler.profile().unwrap();
        println!("{}", profile);
    }
}
