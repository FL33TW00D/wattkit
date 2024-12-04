use pyo3::prelude::*;
use wattkit::{PowerProfile, Sampling, StartStopSampler};

#[pyclass]
struct Profiler {
    sampler: StartStopSampler,
    sample_duration: u64,
    num_samples: usize,
}

#[pyclass]
pub struct PyPowerProfile(pub PowerProfile);

#[pymethods]
impl PyPowerProfile {
    fn __str__(&self) -> PyResult<String> {
        Ok(format!("{:?}", self.0))
    }

    #[getter]
    fn total_cpu_millijoules(&self) -> PyResult<u128> {
        Ok(self.0.total_cpu_millijoules)
    }

    #[getter]
    fn total_gpu_millijoules(&self) -> PyResult<u128> {
        Ok(self.0.total_gpu_millijoules)
    }

    #[getter]
    fn total_ane_millijoules(&self) -> PyResult<u128> {
        Ok(self.0.total_ane_millijoules)
    }

    #[getter]
    fn average_cpu_milliwatts(&self) -> PyResult<u64> {
        Ok(self.0.average_cpu_milliwatts)
    }

    #[getter]
    fn average_gpu_milliwatts(&self) -> PyResult<u64> {
        Ok(self.0.average_gpu_milliwatts)
    }

    #[getter]
    fn average_ane_milliwatts(&self) -> PyResult<u64> {
        Ok(self.0.average_ane_milliwatts)
    }

    #[getter]
    fn total_millijoules(&self) -> PyResult<u128> {
        Ok(self.0.total_millijoules)
    }

    #[getter]
    fn average_milliwatts(&self) -> PyResult<u64> {
        Ok(self.0.average_milliwatts)
    }

    #[getter]
    fn total_duration_milliseconds(&self) -> PyResult<u64> {
        Ok(self.0.total_duration_milliseconds)
    }
}

#[pymethods]
impl Profiler {
    #[new]
    fn new(sample_duration: u64, num_samples: usize) -> PyResult<Self> {
        Ok(Profiler {
            sampler: StartStopSampler::new(),
            sample_duration,
            num_samples,
        })
    }

    fn __enter__(mut slf: PyRefMut<'_, Self>) -> PyResult<PyRefMut<'_, Self>> {
        let duration = slf.sample_duration;
        let num_samples = slf.num_samples;
        slf.sampler.start(duration, num_samples).unwrap();
        assert!(slf.sampler.is_sampling());
        Ok(slf)
    }

    #[pyo3(signature = (_exc_type=None, _exc_value=None, _traceback=None))]
    fn __exit__(
        mut slf: PyRefMut<'_, Self>,
        _exc_type: Option<PyObject>,
        _exc_value: Option<PyObject>,
        _traceback: Option<PyObject>,
    ) -> PyResult<bool> {
        slf.sampler.stop().unwrap();
        assert!(!slf.sampler.is_sampling());
        Ok(true)
    }

    fn get_profile(&self) -> PyResult<PyPowerProfile> {
        let profile = self.sampler.profile();
        Ok(PyPowerProfile(profile.unwrap())) //TODO: proper error handling
    }
}

#[pymodule]
fn _wattkit_pyo3(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Profiler>()?;
    Ok(())
}
