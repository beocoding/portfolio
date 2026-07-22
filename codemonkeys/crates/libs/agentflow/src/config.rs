use crate::{Error, Result};
use std::{env, sync::OnceLock};

#[allow(non_snake_case)]
pub struct Config {
    // Web
    pub WEB_FOLDER: String,

}

impl Config {
    fn load_from_env() -> Result<Config> {
        Ok(Config {
            WEB_FOLDER: get_env("SERVICE_WEB_FOLDER")?,
        })
    }
}

pub fn config() -> &'static Config {
    static INSTANCE: OnceLock<Config> = OnceLock::new();

    INSTANCE.get_or_init(|| match Config::load_from_env() {
        Ok(cfg) => cfg,
        Err(ex) => panic!("FATAL - WHILE LOADING CONFIG - Cause: {ex:?}"),
    })
}

fn get_env(name: &'static str) -> Result<String> {
    env::var(name).map_err(|_| Error::ConfigMissingEnv(name))
}