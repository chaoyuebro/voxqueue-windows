//! Exercise the same USB configuration + heartbeat gate used after firmware flashing.
#[path="../../desktop/src-tauri/src/firmware_flash.rs"]
mod firmware_flash;
fn main()->Result<(),Box<dyn std::error::Error>> {
 let flasher=firmware_flash::FirmwareFlasher::default();flasher.retry_configuration()?;
 let deadline=std::time::Instant::now()+std::time::Duration::from_secs(65);let mut previous=String::new();
 while std::time::Instant::now()<deadline {
  let status=flasher.snapshot();if status.phase!=previous {println!("phase={}",status.phase);previous=status.phase.clone();}
  if status.phase=="completed"{println!("result=USB_configuration_and_fresh_authenticated_heartbeat_verified");return Ok(());}
  if status.phase=="configuration_failed" {return Err(status.message.into());}
  std::thread::sleep(std::time::Duration::from_millis(250));
 }
 Err("Recovery status timeout".into())
}
