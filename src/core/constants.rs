use btleplug::api::bleuuid::uuid_from_u16;
use time::macros::format_description;
use uuid::Uuid;

// UUID for the Heart Rate Service.
// pub const HEARTRATE_SERVICE_UUID: Uuid = uuid_from_u16(0x180D);
/// UUID for the Heart Rate Measurement Characteristic.
pub const HEARTRATE_MEASUREMENT_UUID: Uuid = uuid_from_u16(0x2A37);
pub const DATE_TIME_STRING_FORMAT:&str  = "[year]-[month]-[day] [hour]:[minute]";
