// #[derive(Debug, Deserialize, Serialize)]
// struct LocalConfig {
//     serpress: SerpressConfig,
//     services: Vec<ServiceConfig>,
//     domains: Vec<Domain>,
// }

// #[derive(Debug, Deserialize, Serialize)]
// struct SerpressConfig {
//     remote: Url,
//     local: Url,

//     name_kind: NameKind,    
//     alive_time: String,
// }

// #[derive(Debug, Deserialize, Serialize)]
// struct ServiceConfig {
//     name: String,
//     remote: Url,
//     local: Url,
//     directory: Option<String>,
//     path_modifiers: Option<Vec<PathModifier>>,
// }

// #[derive(Debug, Deserialize, Serialize)]
// enum NameKind {
//     Animal,
//     SixChar,
// }