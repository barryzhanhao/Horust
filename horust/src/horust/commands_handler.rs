use crate::horust::bus::BusConnector;
use crate::horust::formats::{ServiceName, ServiceStatus};
use crate::horust::Event;
use anyhow::{anyhow, bail, Result};
use horust_commands_lib::{CommandsHandlerTrait, HorustMsgServiceStatus,HorustChangeServiceStatus};
use std::collections::HashMap;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;
use std::{fs, thread};
use nix::unistd::Pid;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

pub fn spawn(
    bus: BusConnector<Event>,
    uds_path: PathBuf,
    services: Vec<ServiceName>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut commands_handler = CommandsHandler::new(bus, uds_path, services);
        commands_handler.run();
    })
}

struct CommandsHandler {
    bus: BusConnector<Event>,
    services: HashMap<ServiceName, ServiceStatus>,
    services_pids: HashMap<ServiceName, Pid>,
    uds_listener: UnixListener,
    uds_path: PathBuf,
}

impl CommandsHandler {
    fn new(bus: BusConnector<Event>, uds_path: PathBuf, services: Vec<ServiceName>) -> Self {
        let uds_listener = UnixListener::bind(&uds_path).unwrap();
        uds_listener.set_nonblocking(true).unwrap();
        Self {
            bus,
            uds_path,
            uds_listener,
            services_pids: services.clone().into_iter().map(|name| (name, Pid::from_raw(0))).collect(),
            services: services
                .into_iter()
                .map(|s| (s, ServiceStatus::Initial))
                .collect(),
        }
    }
    fn run(&mut self) {
        loop {
            let evs = self.bus.try_get_events();
            for ev in evs {
                match ev {
                    Event::StatusChanged(name, status) => {
                        let k = self.services.get_mut(&name).unwrap();
                        *k = status;
                    }
                    Event::ShuttingDownInitiated(_) => {
                        fs::remove_file(&self.uds_path).unwrap();
                        return;
                    }
                    Event::PidChanged(name, pid) => {
                        let k = self.services_pids.get_mut(&name).unwrap();
                        *k = pid;
                    }
                    _ => {}
                }
            }
            self.accept().unwrap();
            thread::sleep(Duration::from_millis(300));
        }
    }
}

impl CommandsHandlerTrait for CommandsHandler {
    fn get_unix_listener(&mut self) -> &mut UnixListener {
        &mut self.uds_listener
    }
    fn get_service_status(&self, service_name: &str) -> anyhow::Result<HorustMsgServiceStatus> {
        self.services
            .get(service_name)
            .map(from_service_status)
            .ok_or_else(|| anyhow!("Error: service {service_name} not found."))
    }

    fn get_service_info(&self, service_name: &str) -> Result<String> {
        if let Some(&pid) = self.services_pids.get(service_name)  {
            let mut sys = System::new_all();
            thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage()
            );

            if let Some(process) = sys.process(sysinfo::Pid::from(pid.as_raw() as usize)) {
                let info = format!(
                    "service[{}],pid:{},cpu_usage:{}%,memory:{}KB,disk_usage_total_read:{}KB,disk_usage_total_write:{}KB", service_name, pid, process.cpu_usage(), process.memory() / 1024,
                    process.disk_usage().total_read_bytes / 1024, process.disk_usage().total_written_bytes / 1024,
                );

                Ok(info)
            }else {
                Err(anyhow!("Error: service {service_name} process not found."))
            }
        }else {
            Err(anyhow!("Error: service {service_name} pid not found."))
        }
    }

    fn update_service_status(
        &self,
        service_name: &str,
        new_status: HorustChangeServiceStatus,
    ) -> Result<HorustMsgServiceStatus> {
        match self.services.get(service_name) {
            None => bail!("Service {service_name} not found."),
            Some(service_status) if *service_status == ServiceStatus::Running => {
                match  new_status{
                    HorustChangeServiceStatus::Start => {
                        self.bus.send_event(Event::StatusUpdate(service_name.to_string(),ServiceStatus::InKilling));
                        self.bus.send_event(Event::ForceKill(service_name.to_string()));
                        self.bus.send_event(Event::Run(service_name.to_string()))
                    }
                    HorustChangeServiceStatus::Stop => {
                        self.bus.send_event(Event::StatusUpdate(service_name.to_string(),ServiceStatus::InKilling));
                        self.bus.send_event(Event::ForceKill(service_name.to_string()))
                    }
                }
            }
            _ => bail!("Service {service_name} status is not present."),
        };
        self.get_service_status(service_name)
    }
}

fn from_service_status(status: &ServiceStatus) -> HorustMsgServiceStatus {
    match status {
        ServiceStatus::Starting => HorustMsgServiceStatus::Starting,
        ServiceStatus::Started => HorustMsgServiceStatus::Started,
        ServiceStatus::Running => HorustMsgServiceStatus::Running,
        ServiceStatus::InKilling => HorustMsgServiceStatus::Inkilling,
        ServiceStatus::Success => HorustMsgServiceStatus::Success,
        ServiceStatus::Finished => HorustMsgServiceStatus::Finished,
        ServiceStatus::FinishedFailed => HorustMsgServiceStatus::Finishedfailed,
        ServiceStatus::Failed => HorustMsgServiceStatus::Failed,
        ServiceStatus::Initial => HorustMsgServiceStatus::Initial,
    }
}
