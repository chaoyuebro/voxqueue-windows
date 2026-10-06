use easy_codex_host::provisioning::decode_recovery_profile;
#[test]
fn saved_profile_is_validated_before_it_can_reconfigure_a_keyboard() {
 let mut value=serde_json::json!({"ssid":"test-network","password":"test-password","host":"192.168.1.10","port":17333,"device_secret":vec![7;32]});
 let profile=decode_recovery_profile(&serde_json::to_vec(&value).unwrap()).unwrap();
 let payload:serde_json::Value=serde_json::from_slice(&profile.payload_json().unwrap()).unwrap();
 assert_eq!(payload["wifi_ssid"],"test-network"); assert_eq!(payload["audio_host"],"192.168.1.10"); assert_eq!(payload["audio_enabled"],true);
 for host in ["127.0.0.1","0.0.0.0","169.254.1.1"] { value["host"]=host.into();assert!(decode_recovery_profile(&serde_json::to_vec(&value).unwrap()).is_err()); }
 value["host"]="192.168.1.10".into();value["ssid"]="".into();assert!(decode_recovery_profile(&serde_json::to_vec(&value).unwrap()).is_err());
 value["ssid"]="test-network".into();value["device_secret"]=serde_json::json!(vec![0;32]);assert!(decode_recovery_profile(&serde_json::to_vec(&value).unwrap()).is_err());
 assert!(decode_recovery_profile(b"broken").is_err());
}
