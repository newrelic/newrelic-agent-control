//! Handle to a Windows Job Object and the operations shared by the temporary and the permanent
//! ones: assigning processes, querying them and limiting how many can run.

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use windows::Win32::Foundation::{ERROR_MORE_DATA, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows::core::Owned;

/// Error produced by a Windows Job Object operation.
#[derive(thiserror::Error, Debug)]
#[error("{0}")]
pub struct JobObjectError(pub(crate) String);

/// Handle to a Windows Job Object, with the operations shared by every kind of job. Dropping it
/// only closes the handle; [crate::TemporaryJobObject] and [crate::PermanentJobObject] decide what
/// happens to the processes.
pub struct JobHandle {
    pub(crate) handle: Owned<HANDLE>,
}

/// Process counters of a Job Object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobAccounting {
    /// Processes currently in the job.
    pub active_processes: u32,
    /// Processes ever associated with the job, including the ones that already exited.
    pub total_processes: u32,
}

impl JobHandle {
    /// Assigns the given process to this JobObject. Processes it creates join the job too.
    pub fn assign_process(&self, process: &Child) -> Result<(), JobObjectError> {
        unsafe {
            let process_handle = HANDLE(process.as_raw_handle());
            AssignProcessToJobObject(*self.handle, process_handle)
        }
        .map_err(|e| JobObjectError(format!("assigning process to JobObject: {e}")))
    }

    /// Terminates all processes in the JobObject.
    pub(crate) fn terminate(&self) -> Result<(), JobObjectError> {
        unsafe { TerminateJobObject(*self.handle, 0) }
            .map_err(|e| JobObjectError(format!("terminating JobObject: {e}")))
    }

    /// Returns the process counters of the JobObject.
    pub fn accounting(&self) -> Result<JobAccounting, JobObjectError> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(*self.handle),
                JobObjectBasicAccountingInformation,
                (&raw mut info).cast(),
                size_of_val(&info) as u32,
                None,
            )
        }
        .map_err(|e| JobObjectError(format!("querying JobObject accounting: {e}")))?;
        Ok(JobAccounting {
            active_processes: info.ActiveProcesses,
            total_processes: info.TotalProcesses,
        })
    }

    /// Returns the ids of the processes currently in the JobObject.
    pub fn process_ids(&self) -> Result<Vec<u32>, JobObjectError> {
        // The list is two `u32` counters followed by `usize` ids, so a `usize` buffer keeps it
        // aligned and the ids start at index `HEADER`.
        const HEADER: usize = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList)
            / size_of::<usize>();

        let mut process_id_capacity = INITIAL_PROCESS_ID_CAPACITY;
        loop {
            let mut buffer = vec![0usize; HEADER + process_id_capacity];
            let result = unsafe {
                QueryInformationJobObject(
                    Some(*self.handle),
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    size_of_val(buffer.as_slice()) as u32,
                    None,
                )
            };
            // Safety: the buffer is aligned and large enough for the counters.
            let list = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            match result {
                Ok(()) => {
                    let ids = &buffer[HEADER..][..list.NumberOfProcessIdsInList as usize];
                    return Ok(ids.iter().map(|&id| id as u32).collect());
                }
                // Processes may join between queries, so it retries until the list fits.
                Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() => {
                    process_id_capacity =
                        (list.NumberOfAssignedProcesses as usize).max(process_id_capacity * 2);
                }
                Err(e) => {
                    return Err(JobObjectError(format!(
                        "querying JobObject process ids: {e}"
                    )));
                }
            }
        }
    }

    /// Limits the processes that can be active in the JobObject at once to `max`. Once the limit
    /// is reached, creating or assigning another process fails. Processes already in the job
    /// are not affected, and `0` blocks any new process. The other limits of the job are kept.
    pub fn limit_active_processes(&self, max: u32) -> Result<(), JobObjectError> {
        let mut limits = self.extended_limit_information()?;
        // Added to the existing flags, so that e.g. "kill on job close" is kept.
        limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = max;
        self.set_extended_limit_information(&limits)
    }

    /// Replaces the extended limit information of the JobObject, which holds all its limits:
    /// `LimitFlags` says which ones are enabled, so setting one limit without the others disables
    /// them.
    pub(crate) fn set_extended_limit_information(
        &self,
        limits: &JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    ) -> Result<(), JobObjectError> {
        unsafe {
            SetInformationJobObject(
                *self.handle,
                JobObjectExtendedLimitInformation,
                (limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(limits) as u32,
            )
        }
        .map_err(|e| JobObjectError(format!("setting JobObject limits: {e}")))
    }

    /// Returns the extended limit information of the JobObject, which holds all its limits.
    fn extended_limit_information(
        &self,
    ) -> Result<JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectError> {
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(*self.handle),
                JobObjectExtendedLimitInformation,
                (&raw mut limits).cast(),
                size_of_val(&limits) as u32,
                None,
            )
        }
        .map_err(|e| JobObjectError(format!("querying JobObject limits: {e}")))?;
        Ok(limits)
    }
}

/// Process ids [JobHandle::process_ids] makes room for in its first query. Any value is correct,
/// since a larger list is retried with the reported size. 16 fits a sub-agent and a few of its
/// children in a single query.
const INITIAL_PROCESS_ID_CAPACITY: usize = 16;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::TemporaryJobObject;
    use rstest::rstest;
    use std::process::{Command, Stdio};
    use std::time::Duration;
    use windows::Win32::System::JobObjects::JOB_OBJECT_LIMIT;
    use windows::Win32::System::JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    #[rstest]
    #[case::empty(0)]
    #[case::one_process(1)]
    #[case::beyond_initial_capacity(INITIAL_PROCESS_ID_CAPACITY + 1)]
    fn test_accounting_and_process_ids(#[case] processes: usize) {
        let job = TemporaryJobObject::new().unwrap();
        let children: Vec<Child> = (0..processes)
            .map(|_| {
                let child = long_running_process();
                job.assign_process(&child).unwrap();
                child
            })
            .collect();

        let expected = JobAccounting {
            active_processes: processes as u32,
            total_processes: processes as u32,
        };
        assert_eq!(job.accounting().unwrap(), expected);
        assert_eq!(sorted(job.process_ids().unwrap()), sorted(ids(&children)));

        kill_and_wait(&job, children);
    }

    #[test]
    fn test_terminate_kills_the_descendants_that_joined_the_job() {
        let job = TemporaryJobObject::new().unwrap();
        let mut child = Command::new("cmd")
            .args(["/C", "ping -n 16 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        job.assign_process(&child).unwrap();

        let descendant_joined = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            job.accounting().unwrap().active_processes == 2
        });
        assert!(descendant_joined, "{:?}", job.accounting().unwrap());
        let ids = job.process_ids().unwrap();
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert!(ids.contains(&child.id()), "{ids:?}");

        job.terminate().unwrap();

        // The descendant is not a child of this process, so the job is the only way to see it exit.
        let all_exited = (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            job.accounting().unwrap().active_processes == 0
        });
        assert!(all_exited, "{:?}", job.accounting().unwrap());
        assert!(
            exits_within(&mut child, EXIT_TIMEOUT),
            "process kept running"
        );
    }

    #[rstest]
    #[case::below_the_limit(2, true)]
    #[case::at_the_limit(1, false)]
    #[case::zero(0, false)]
    fn test_limit_active_processes(#[case] max: u32, #[case] accepts_another: bool) {
        let job = TemporaryJobObject::new().unwrap();
        let first = long_running_process();
        job.assign_process(&first).unwrap();

        job.limit_active_processes(max).unwrap();
        let mut second = long_running_process();

        assert_eq!(job.assign_process(&second).is_ok(), accepts_another);
        let expected_active = if accepts_another { 2 } else { 1 };
        assert_eq!(job.accounting().unwrap().active_processes, expected_active);
        assert!(has_flag(&job, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE));
        let _ = second.kill();
        kill_and_wait(&job, vec![first, second]);
    }

    /// `ping` runs for ~15s without a console, unlike `timeout` which exits right away without one.
    pub(crate) fn long_running_process() -> Child {
        Command::new("ping")
            .args(["-n", "16", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .expect("Failed to spawn process")
    }

    /// How long a killed process gets to exit.
    pub(crate) const EXIT_TIMEOUT: Duration = Duration::from_secs(2);

    pub(crate) fn exits_within(child: &mut Child, timeout: Duration) -> bool {
        let step = Duration::from_millis(50);
        (0..timeout.as_millis() / step.as_millis()).any(|_| {
            if child.try_wait().is_ok_and(|status| status.is_some()) {
                return true;
            }
            std::thread::sleep(step);
            false
        })
    }

    pub(crate) fn kill_and_wait(job: &JobHandle, mut children: Vec<Child>) {
        job.terminate().unwrap();
        for child in &mut children {
            assert!(exits_within(child, EXIT_TIMEOUT), "process kept running");
        }
    }

    fn ids(children: &[Child]) -> Vec<u32> {
        children.iter().map(Child::id).collect()
    }

    fn sorted(mut ids: Vec<u32>) -> Vec<u32> {
        ids.sort();
        ids
    }

    fn has_flag(job: &JobHandle, flag: JOB_OBJECT_LIMIT) -> bool {
        let flags = job
            .extended_limit_information()
            .unwrap()
            .BasicLimitInformation
            .LimitFlags;
        flags.0 & flag.0 != 0
    }
}
