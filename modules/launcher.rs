// AGPL-3.0 许可证

use std::io::{Error, ErrorKind, Result};
use std::thread;
use std::time::Duration;
use uuid::Uuid;
use windows::Win32::Foundation::{VARIANT_BOOL};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, IRegisteredTaskCollection, IRunningTask, ITaskDefinition, ITaskFolder, ITaskService, ITaskSettings, TASK_ACTION_EXEC,
    TASK_CREATE_OR_UPDATE, TASK_LOGON_SERVICE_ACCOUNT, TASK_RUN_USE_SESSION_ID, TASK_STATE_RUNNING,
    TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, Interface};

const TASK_NAME_PREFIX: &str = "AkiSpace_Launch_";
const TASK_FOLDER: &str = "\\";
const POLL_INTERVAL_MS: u64 = 200;
const MAX_POLL_MS: u64 = 2000;

fn to_bstr(s: &str) -> BSTR {
    BSTR::from(s)
}

fn to_variant_bool(b: bool) -> VARIANT_BOOL {
    if b { VARIANT_BOOL(-1) } else { VARIANT_BOOL(0) }
}

fn empty_variant() -> VARIANT {
    VARIANT::default()
}

pub fn launch_in_session(session_id: u32, exe_path: &str, args: &str) -> Result<u32> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;

        let task_service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| Error::new(ErrorKind::Other, format!("创建 TaskScheduler 失败: {:?}", e)))?;

        let empty = empty_variant();
        task_service.Connect(&empty, &empty, &empty, &empty)
            .map_err(|e| Error::new(ErrorKind::Other, format!("连接 TaskScheduler 失败: {:?}", e)))?;

        let root_folder: ITaskFolder = task_service.GetFolder(&to_bstr(TASK_FOLDER))
            .map_err(|e| Error::new(ErrorKind::Other, format!("获取任务文件夹失败: {:?}", e)))?;

        let task_name = format!("{}{}", TASK_NAME_PREFIX, Uuid::new_v4().simple());
        let task_name_bstr = to_bstr(&task_name);

        let task_def: ITaskDefinition = task_service.NewTask(0)
            .map_err(|e| Error::new(ErrorKind::Other, format!("创建任务定义失败: {:?}", e)))?;

        let action_collection = task_def.Actions()
            .map_err(|e| Error::new(ErrorKind::Other, format!("获取动作集合失败: {:?}", e)))?;

        let action = action_collection.Create(TASK_ACTION_EXEC)
            .map_err(|e| Error::new(ErrorKind::Other, format!("创建执行动作失败: {:?}", e)))?;

        let exec_action: IExecAction = action.cast()
            .map_err(|e| Error::new(ErrorKind::Other, format!("转换为 IExecAction 失败: {:?}", e)))?;

        exec_action.SetPath(&to_bstr(exe_path))
            .map_err(|e| Error::new(ErrorKind::Other, format!("设置执行路径失败: {:?}", e)))?;

        if !args.is_empty() {
            exec_action.SetArguments(&to_bstr(args))
                .map_err(|e| Error::new(ErrorKind::Other, format!("设置参数失败: {:?}", e)))?;
        }

        let settings: ITaskSettings = task_def.Settings()
            .map_err(|e| Error::new(ErrorKind::Other, format!("获取任务设置失败: {:?}", e)))?;

        settings.SetHidden(to_variant_bool(true))
            .map_err(|e| Error::new(ErrorKind::Other, format!("设置隐藏失败: {:?}", e)))?;

        settings.SetDeleteExpiredTaskAfter(&to_bstr("PT60S"))
            .map_err(|e| Error::new(ErrorKind::Other, format!("设置过期删除失败: {:?}", e)))?;

        let empty = empty_variant();
        let registered_task: IRegisteredTask = root_folder.RegisterTaskDefinition(
            &task_name_bstr,
            &task_def,
            TASK_CREATE_OR_UPDATE.0,
            &empty,
            &empty,
            TASK_LOGON_SERVICE_ACCOUNT,
            &empty,
        ).map_err(|e| Error::new(ErrorKind::Other, format!("注册任务失败: {:?}", e)))?;

        let empty = empty_variant();
        let running_task: IRunningTask = registered_task.RunEx(&empty, TASK_RUN_USE_SESSION_ID.0, session_id as i32, &BSTR::from(""))
            .map_err(|e| Error::new(ErrorKind::Other, format!("运行任务失败: {:?}", e)))?;

        let mut waited = 0u64;
        let mut task_pid = 0u32;

        while waited < MAX_POLL_MS {
            let state = running_task.State()
                .map_err(|e| Error::new(ErrorKind::Other, format!("获取任务状态失败: {:?}", e)))?;

            if state == TASK_STATE_RUNNING {
                task_pid = running_task.EnginePID()
                    .map_err(|e| Error::new(ErrorKind::Other, format!("获取进程 ID 失败: {:?}", e)))?;
                break;
            }

            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            waited += POLL_INTERVAL_MS;
        }

        if task_pid == 0 {
            let _ = root_folder.DeleteTask(&task_name_bstr, 0);
            return Err(Error::new(ErrorKind::TimedOut, "任务启动超时"));
        }

        let _ = root_folder.DeleteTask(&task_name_bstr, 0);

        drop(running_task);
        drop(registered_task);
        drop(root_folder);
        drop(task_def);
        drop(action_collection);
        drop(action);
        drop(exec_action);
        drop(settings);
        drop(task_service);

        CoUninitialize();

        Ok(task_pid)
    }
}

pub fn cleanup_orphaned_tasks() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;

        let task_service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| Error::new(ErrorKind::Other, format!("创建 TaskScheduler 失败: {:?}", e)))?;

        let empty = empty_variant();
        task_service.Connect(&empty, &empty, &empty, &empty)
            .map_err(|e| Error::new(ErrorKind::Other, format!("连接 TaskScheduler 失败: {:?}", e)))?;

        let root_folder: ITaskFolder = task_service.GetFolder(&to_bstr(TASK_FOLDER))
            .map_err(|e| Error::new(ErrorKind::Other, format!("获取任务文件夹失败: {:?}", e)))?;

        let tasks: IRegisteredTaskCollection = root_folder.GetTasks(0)
            .map_err(|e| Error::new(ErrorKind::Other, format!("获取任务列表失败: {:?}", e)))?;

        let count = tasks.Count()
            .map_err(|e| Error::new(ErrorKind::Other, format!("获取任务数量失败: {:?}", e)))?;

        for i in 0..count {
            let index = VARIANT::from(i + 1);
            let task: IRegisteredTask = tasks.get_Item(&index)
                .map_err(|e| Error::new(ErrorKind::Other, format!("获取任务项失败: {:?}", e)))?;

            let name = task.Name()
                .map_err(|e| Error::new(ErrorKind::Other, format!("获取任务名称失败: {:?}", e)))?;

            let name_str = name.to_string();

            if name_str.starts_with(TASK_NAME_PREFIX) {
                let name_bstr = to_bstr(&name_str);
                let _ = root_folder.DeleteTask(&name_bstr, 0);
            }

            drop(task);
        }

        drop(tasks);
        drop(root_folder);
        drop(task_service);

        CoUninitialize();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_name_format() {
        let name = format!("{}{}", TASK_NAME_PREFIX, Uuid::new_v4().simple());
        assert!(name.starts_with(TASK_NAME_PREFIX));
        assert!(name.len() > TASK_NAME_PREFIX.len());
    }
}