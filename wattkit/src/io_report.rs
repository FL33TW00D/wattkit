use std::{
    marker::{PhantomData, PhantomPinned},
    mem::MaybeUninit,
};

use crate::cf_utils::*;
use core_foundation::{
    array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef},
    base::{kCFAllocatorDefault, CFRelease, CFTypeRef},
    dictionary::{
        CFDictionaryCreateMutableCopy, CFDictionaryGetCount, CFDictionaryRef,
        CFMutableDictionaryRef,
    },
    string::CFStringRef,
};

#[derive(Debug, thiserror::Error)]
pub enum IOReportError {
    #[error("Failed to get channels")]
    ChannelError,
    #[error("Failed to create subscription")]
    SubscriptionError,
}

pub type CVoidRef = *const std::ffi::c_void;
type Result<T> = std::result::Result<T, IOReportError>;

#[repr(C)]
pub struct IOReportSubscription {
    _data: [u8; 0],
    _phantom: PhantomData<(*mut u8, PhantomPinned)>,
}

pub type IOReportSubscriptionRef = *const IOReportSubscription;

#[link(name = "IOReport", kind = "dylib")]
#[rustfmt::skip]
extern "C" {
  pub fn IOReportCopyAllChannels(a: u64, b: u64) -> CFMutableDictionaryRef;
  pub fn IOReportCopyChannelsInGroup(group: CFStringRef, subgroup: CFStringRef, c: u64, d: u64, e: u64) -> CFMutableDictionaryRef;
  pub fn IOReportMergeChannels(a: CFDictionaryRef, b: CFDictionaryRef, nil: CFTypeRef);
  pub fn IOReportCreateSubscription(a: CVoidRef, desired_channels: CFMutableDictionaryRef, subbed_channels: *mut CFMutableDictionaryRef, channel_id: u64, b: CFTypeRef) -> IOReportSubscriptionRef;
  pub fn IOReportCreateSamples(a: IOReportSubscriptionRef, b: CFMutableDictionaryRef, c: CFTypeRef) -> CFDictionaryRef;
  pub fn IOReportCreateSamplesDelta(a: CFDictionaryRef, b: CFDictionaryRef, c: CFTypeRef) -> CFDictionaryRef;
  pub fn IOReportChannelGetGroup(a: CFDictionaryRef) -> CFStringRef;
  pub fn IOReportChannelGetSubGroup(a: CFDictionaryRef) -> CFStringRef;
  pub fn IOReportChannelGetChannelName(a: CFDictionaryRef) -> CFStringRef;
  pub fn IOReportSimpleGetIntegerValue(a: CFDictionaryRef, b: *mut i32) -> i64;
  pub fn IOReportChannelGetUnitLabel(a: CFDictionaryRef) -> CFStringRef;
  pub fn IOReportSampleCopyDescription(a: CFDictionaryRef, b: i64) -> CFStringRef;
  pub fn IOReportStateGetCount(a: CFDictionaryRef) -> i32;
  pub fn IOReportStateGetNameForIndex(a: CFDictionaryRef, b: i32) -> CFStringRef;
  pub fn IOReportStateGetResidency(a: CFDictionaryRef, b: i32) -> i64;
  pub fn IOReportArrayGetValueAtIndex(a: CFDictionaryRef, b: i64) -> u64;
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
pub enum EnergyUnit {
    MilliJoules,
    MicroJoules,
    NanoJoules,
}

impl std::fmt::Display for EnergyUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MilliJoules => write!(f, "mJ"),
            Self::MicroJoules => write!(f, "μJ"), //careful, u != μ but goddamn it looks better
            Self::NanoJoules => write!(f, "nJ"),
        }
    }
}

impl<S: AsRef<str>> From<S> for EnergyUnit {
    fn from(s: S) -> Self {
        match s.as_ref() {
            "mJ" => Self::MilliJoules,
            "uJ" => Self::MicroJoules,
            "nJ" => Self::NanoJoules,
            _ => panic!("Invalid energy unit: {}", s.as_ref()),
        }
    }
}

pub struct IOReportIterator {
    sample: CFDictionaryRef,
    index: isize,
    channels: CFArrayRef,
    num_channels: isize,
}

impl IOReportIterator {
    pub fn new(data: CFDictionaryRef) -> Self {
        let channels = cfdict_get_val(data, "IOReportChannels").unwrap() as CFArrayRef;
        let num_channels = unsafe { CFArrayGetCount(channels) } as isize;
        Self {
            sample: data,
            channels,
            num_channels,
            index: 0,
        }
    }
}

impl Drop for IOReportIterator {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.sample as _);
        }
    }
}

#[derive(Debug)]
pub enum IOReportChannelGroup {
    EnergyModel,
    CPUStats,
    GPUStats,
    H11ANE,
    SoCStats,
    Unknown(String),
}

impl IOReportChannelGroup {
    pub fn as_str(&self) -> &str {
        match self {
            Self::EnergyModel => "Energy Model",
            Self::CPUStats => "CPU Stats",
            Self::GPUStats => "GPU Stats",
            Self::H11ANE => "H11ANE",
            Self::SoCStats => "SoC Stats",
            Self::Unknown(s) => s.as_str(),
        }
    }
}

impl<S: AsRef<str>> From<S> for IOReportChannelGroup {
    fn from(s: S) -> Self {
        match s.as_ref() {
            "Energy Model" => Self::EnergyModel,
            "CPU Stats" => Self::CPUStats,
            "GPU Stats" => Self::GPUStats,
            "H11ANE" => Self::H11ANE,
            "SoC Stats" => Self::SoCStats,
            s => Self::Unknown(s.to_string()),
        }
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq)]
pub enum IOReportChannelName {
    CPUEnergy,
    GPUEnergy,
    ANE,
    Unknown(String),
}

impl IOReportChannelName {
    pub fn as_str(&self) -> &str {
        match self {
            Self::CPUEnergy => "CPU Energy",
            Self::GPUEnergy => "GPU Energy",
            Self::ANE => "ANE",
            Self::Unknown(s) => s.as_str(),
        }
    }
}

impl From<String> for IOReportChannelName {
    fn from(s: String) -> Self {
        match s.as_str() {
            "CPU Energy" => Self::CPUEnergy,
            "GPU Energy" => Self::GPUEnergy,
            c if c.starts_with("ANE") => Self::ANE,
            s => Self::Unknown(s.to_string()),
        }
    }
}

impl std::fmt::Display for IOReportChannelName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct IOReportIteratorItem {
    pub group: IOReportChannelGroup,
    pub subgroup: String,
    pub channel_name: IOReportChannelName,
    pub unit: String,
    pub item: CFDictionaryRef,
}

impl Iterator for IOReportIterator {
    type Item = IOReportIteratorItem;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.num_channels {
            return None;
        }

        let item = unsafe { CFArrayGetValueAtIndex(self.channels, self.index) } as CFDictionaryRef;

        let group =
            IOReportChannelGroup::from(get_cf_string(|| unsafe { IOReportChannelGetGroup(item) }));
        let subgroup = get_cf_string(|| unsafe { IOReportChannelGetSubGroup(item) });
        let channel = IOReportChannelName::from(get_cf_string(|| unsafe {
            IOReportChannelGetChannelName(item)
        }));
        let unit = from_cfstr(unsafe { IOReportChannelGetUnitLabel(item) })
            .trim()
            .to_string();

        self.index += 1;
        Some(IOReportIteratorItem {
            group,
            subgroup,
            channel_name: channel,
            unit,
            item,
        })
    }
}

pub struct IOReportSample {
    iterator: IOReportIterator,
    duration: u64,
}

impl IOReportSample {
    pub fn iterator(&self) -> &IOReportIterator {
        &self.iterator
    }

    pub fn iterator_mut(&mut self) -> &mut IOReportIterator {
        &mut self.iterator
    }

    pub fn duration(&self) -> u64 {
        self.duration
    }
}

impl IOReportSample {
    pub fn new(iterator: IOReportIterator, duration: u64) -> Self {
        Self { iterator, duration }
    }
}

pub struct IOReportChannelRequest {
    pub group: IOReportChannelGroup,
    pub subgroup: Option<String>,
}

impl IOReportChannelRequest {
    pub fn new<S: ToString>(group: IOReportChannelGroup, subgroup: Option<S>) -> Self {
        Self {
            group,
            subgroup: subgroup.map(|s| s.to_string()),
        }
    }
}

#[derive(Debug)]
pub struct IOReport {
    subscription: IOReportSubscriptionRef,
    channels: CFMutableDictionaryRef,
    previous: Option<(CFDictionaryRef, std::time::Instant)>,
}

impl IOReport {
    pub fn new(channels: Vec<IOReportChannelRequest>) -> Result<Self> {
        let channels = Self::create_channels(channels)?;
        let subscription = Self::subscribe(channels)?;

        Ok(Self {
            subscription,
            channels,
            previous: None,
        })
    }

    fn subscribe(channel: CFMutableDictionaryRef) -> Result<IOReportSubscriptionRef> {
        let mut subscription: MaybeUninit<CFMutableDictionaryRef> = MaybeUninit::uninit();
        let sub_ref = unsafe {
            IOReportCreateSubscription(
                std::ptr::null(),
                channel,
                subscription.as_mut_ptr(),
                0,
                std::ptr::null(),
            )
        };
        if sub_ref.is_null() {
            return Err(IOReportError::SubscriptionError);
        }

        unsafe { subscription.assume_init() };
        Ok(sub_ref)
    }

    fn create_channels(
        channel_reqs: Vec<IOReportChannelRequest>,
    ) -> Result<CFMutableDictionaryRef> {
        // if no items are provided, return all channels
        if channel_reqs.is_empty() {
            unsafe {
                let c = IOReportCopyAllChannels(0, 0);
                let dict_ref =
                    CFDictionaryCreateMutableCopy(kCFAllocatorDefault, CFDictionaryGetCount(c), c);
                CFRelease(c as _);
                return Ok(dict_ref);
            }
        }

        let mut channels = Vec::with_capacity(channel_reqs.len());
        for request in channel_reqs {
            let gname = cfstr(request.group.as_str());
            let sname = request.subgroup.as_deref().map_or(std::ptr::null(), cfstr);
            let chan = unsafe { IOReportCopyChannelsInGroup(gname, sname, 0, 0, 0) };
            channels.push(chan);

            unsafe { CFRelease(gname as _) };
            if request.subgroup.is_some() {
                unsafe { CFRelease(sname as _) };
            }
        }

        let base_channel = channels[0];
        for channel in channels.iter().skip(1) {
            unsafe { IOReportMergeChannels(base_channel, *channel, std::ptr::null()) };
        }

        let size = unsafe { CFDictionaryGetCount(base_channel) };
        let chan_dict_ref =
            unsafe { CFDictionaryCreateMutableCopy(kCFAllocatorDefault, size, base_channel) };

        for channel in channels {
            unsafe { CFRelease(channel as _) };
        }

        if cfdict_get_val(chan_dict_ref, "IOReportChannels").is_none() {
            return Err(IOReportError::ChannelError);
        }

        Ok(chan_dict_ref)
    }

    fn initial_sample(&self) -> (CFDictionaryRef, std::time::Instant) {
        (
            unsafe { IOReportCreateSamples(self.subscription, self.channels, std::ptr::null()) },
            std::time::Instant::now(),
        )
    }

    pub fn get_samples(&mut self, duration: u64, count: usize) -> Vec<IOReportSample> {
        let mut samples: Vec<IOReportSample> = Vec::with_capacity(count);
        let step_msec = duration / count as u64;

        let mut prev = match self.previous {
            Some(x) => x,
            None => self.initial_sample(),
        };

        for _ in 0..count {
            std::thread::sleep(std::time::Duration::from_millis(step_msec));

            let next = self.initial_sample();
            let diff = unsafe { IOReportCreateSamplesDelta(prev.0, next.0, std::ptr::null()) };
            unsafe { CFRelease(prev.0 as _) };

            let elapsed = next.1.duration_since(prev.1).as_millis() as u64;
            prev = next;

            samples.push(IOReportSample::new(
                IOReportIterator::new(diff),
                elapsed.max(1),
            ));
        }

        self.previous = Some(prev);
        samples
    }
}

impl Drop for IOReport {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.channels as _);
            CFRelease(self.subscription as _);
            if self.previous.is_some() {
                CFRelease(self.previous.unwrap().0 as _);
            }
        }
    }
}
