use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;
use std::string::String;
use reqwest::Url;
use serde::Deserialize;
use serde_json::{json, Value};
use super::{NksProvider};
use tracing::instrument;
use tokio::runtime::Runtime;

use crate::common::{
    crypto::{
        algorithms::{
            encryption::AsymmetricEncryption,
        },
    },
    error::SecurityModuleError,
    traits::module_provider::Provider,
};
use crate::common::traits::module_provider_config::ProviderConfig;
use crate::nks::NksConfig;


/// Implements the `Provider` trait, providing cryptographic operations utilizing a nks.


impl Provider for NksProvider {
    /// Creates a new cryptographic key identified by `key_id` within the NksProvider.
    ///
    /// This function generates a new cryptographic key within the NksProvider, using the settings
    /// specified in the `config` parameter. The key is made persistent and associated with the provided `key_id`.
    ///
    /// # Arguments
    ///
    /// * `key_id` - A string slice that uniquely identifies the key to be created.
    /// * `config` - A Box containing a `ProviderConfig` object that specifies the settings for the key.
    ///
    /// # Returns
    ///
    /// A `Result` that, on success, contains `Ok(())`, indicating that the key was created successfully.
    /// On failure, it returns a `SecurityModuleError`.
    ///
    /// # Example
    ///
    /// ```
    /// let config = get_config("rsa").unwrap();
    /// provider.create_key("test_rsa_key", Box::new(config.clone())).expect("Failed to create RSA key");
    /// ```
    #[instrument]
    fn create_key(&mut self, key_id: &str, config: Box<dyn ProviderConfig>) -> Result<(), SecurityModuleError> {
        if let Some(nks_config) = config.as_any().downcast_ref::<NksConfig>() {
            let runtime = Runtime::new().unwrap();
            let get_and_save_keypair_result = runtime.block_on(get_and_save_key_pair(
                &*nks_config.nks_token.clone(),
                key_id,
                match nks_config.key_algorithm.clone() {
                    AsymmetricEncryption::Rsa(_) => "rsa",
                    AsymmetricEncryption::Ecc(_) => "ecdsa",
                    //TODO: add match for ecdh
                },
                Url::parse(&nks_config.nks_address).unwrap()
            ));
            match get_and_save_keypair_result {
                Ok((result_string, new_token)) => {
                    println!("Key pair generated and saved successfully: {}", result_string);
                    self.secrets_json = Some(result_string.parse().unwrap());
                    //safe token to config
                    let config = NksConfig::new(
                        new_token.clone(),
                        nks_config.nks_address.clone(),
                        nks_config.key_algorithm.clone(),
                        nks_config.hash.clone(),
                        nks_config.key_usages.clone(),
                    );
                    self.config = Some(config);
                    //save token in token.json for persistence
                    let token_data = json!({
                     "user_token": new_token.clone()
                     });
                    fs::write("token.json", token_data.to_string()).expect("Error writing to token.json");

                    println!("Secrets: {:?}", self.secrets_json);
                    println!("Token: {}", new_token);
                    Ok(())
                }
                Err(err) => {
                    println!("Failed to generate and save key pair: {}", err);
                    Err(SecurityModuleError::NksError)
                }
            }
        } else {
            println!("Failed to downcast to NksConfig");
            Err(SecurityModuleError::NksError)
        }

    }

    /// Loads an existing cryptographic key identified by `key_id` from the NksProvider.
    ///
    /// This function retrieves an existing cryptographic key from the NksProvider, using the settings
    /// specified in the `config` parameter. The key is associated with the provided `key_id`.
    ///
    /// # Arguments
    ///
    /// * `key_id` - A string slice that uniquely identifies the key to be loaded.
    /// * `config` - A Box containing a `ProviderConfig` object that specifies the settings for the key.
    ///
    /// # Returns
    ///
    /// A `Result` that, on success, contains `Ok(())`, indicating that the key was loaded successfully.
    /// On failure, it returns a `SecurityModuleError`.
    ///
    /// # Example
    ///
    /// ```
    /// let config = get_config("rsa").unwrap();
    /// provider.load_key("test_rsa_key", Box::new(config.clone())).expect("Failed to load RSA key");
    /// ```
    fn load_key(&mut self, key_id: &str, _config: Box<dyn ProviderConfig>) -> Result<(), SecurityModuleError> {
        // Check if secrets_json is None
        if let Some(secrets_json) = &self.secrets_json {
            // Iterate over the secrets_json object
            if let Some(keys) = secrets_json.get("keys") {
                for key in keys.as_array().unwrap() {
                    // Check if the key_id matches
                    if key.get("id").unwrap().as_str().unwrap() == key_id {
                        // Set the public_key and private_key
                        self.public_key = key.get("publicKey").unwrap().as_str().unwrap().to_string();
                        self.private_key = key.get("privateKey").unwrap().as_str().unwrap().to_string();
                        println!("Public Key: {}", self.public_key);
                        println!("Private Key: {}", self.private_key);
                        return Ok(());
                    }
                }
            }
        } else {
            println!("Secrets JSON is empty");
            return Err(SecurityModuleError::NksError);
        }

        // If no matching key is found, return an error
        println!("Key '{}' not found in secrets_json", key_id);
        Err(SecurityModuleError::NksError)
    }

    /// Initializes the NksProvider module and prepares it for further cryptographic operations.
    ///
    /// This function sets up the NksProvider context by loading the configuration, establishing a connection with the Nks server,
    /// and retrieving the current secrets. It should be called before performing any other operations with the NksProvider.
    ///
    /// # Returns
    ///
    /// A `Result` that, on success, contains `Ok(())`, indicating that the module was initialized successfully.
    /// On failure, it returns a `SecurityModuleError`.
    ///
    /// # Example
    ///
    /// ```
    /// provider.initialize_module().expect("Failed to initialize module");
    /// ```
    #[instrument]
    fn initialize_module(&mut self) -> Result<(), SecurityModuleError> {
        if let Some(nks_config) = self.config.as_ref().unwrap().as_any().downcast_ref::<NksConfig>() {
            //get address and token from config
            let nks_address_str = nks_config.nks_address.clone();
            let nks_address = Some(Url::from_str(nks_address_str.as_str()).unwrap());
            let mut nks_token = nks_config.nks_token.clone();
            if nks_token.is_empty() {
                println!("Token field in config is empty. checking for token.json...");
                // Check if token file exists
                let tokens_file_path = Box::new(Path::new("token.json")); // Adjust the path as needed
                if Path::new(&*tokens_file_path).exists() {
                    println!("Tokens file exists.");
                    nks_token = get_user_token_from_file().unwrap();
                } else {
                    println!("Token file does not exist. Generating token...");
                    // Token field empty and no token in token.json, generate token using API
                    let runtime = Runtime::new().unwrap();
                    let nks_address = nks_address.clone().ok_or(SecurityModuleError::NksError)?;
                    match runtime.block_on(get_token(nks_address.clone())) {
                        Ok(token) => {
                            nks_token = token;
                        }
                        Err(err) => {
                            println!("Failed to get tokens from API: {}", err);
                            return Err(SecurityModuleError::NksError);
                        }
                    }
                }
            }
            //store current secrets
            let runtime = Runtime::new().unwrap();
            match runtime.block_on(get_secrets(&nks_token.as_str(), &nks_address_str)) {
                Ok((secrets_json, new_token)) => {
                    self.secrets_json = Some(secrets_json.parse().unwrap());
                    nks_token = new_token;
                }
                Err(err) => {
                    println!("Failed to get secrets: {}", err);
                    return Err(SecurityModuleError::NksError);
                }
            }
            //safe token to config
            let config = NksConfig::new(
                nks_token.clone(),
                nks_config.nks_address.clone(),
                nks_config.key_algorithm.clone(),
                nks_config.hash.clone(),
                nks_config.key_usages.clone(),
            );
            self.config = Some(config);
            //save token in token.json for persistence
            let token_data = json!({
                "user_token": nks_token.clone()
            });
            fs::write("token.json", token_data.to_string()).expect("Error writing to token.json");
            println!("Nks initialized successfully.");
            println!("Secrets: {:?}", self.secrets_json);
            Ok(())
        } else {
            println!("Failed to downcast to NksConfig");
            Err(SecurityModuleError::NksError)
        }
    }
}

/// Retrieves the user token from the `token.json` file.
///
/// This function opens the `token.json` file and reads its contents. It then parses the contents as JSON and retrieves the `user_token` field.
///
/// # Returns
///
/// An `Option<String>` that, if the file exists and the `user_token` field is found, contains the user token as a `String`.
/// If the file does not exist, or the `user_token` field is not found, it returns `None`.
///
/// # Example
///
/// ```
/// let user_token = get_user_token_from_file();
/// if let Some(token) = user_token {
///     println!("User token: {}", token);
/// } else {
///     println!("User token not found");
/// }
/// ```
fn get_user_token_from_file() -> Option<String> {
    let mut file = File::open("token.json").ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;

    let json: Value = serde_json::from_str(&contents).ok()?;

    if let Some(user_token) = json["user_token"].as_str() {
        Some(user_token.to_string())
    } else {
        println!("user_token not found or invalid format.");
        Some("no valid token".to_string())
    }
}

/// Retrieves a user token from the NksProvider.
///
/// This asynchronous function sends a GET request to the NksProvider's `getToken` endpoint.
/// It then parses the JSON response and retrieves the `token` field.
///
/// # Arguments
///
/// * `nks_address` - A `Url` that specifies the address of the NksProvider.
///
/// # Returns
///
/// A `Result` that, on success, contains an `Ok(String)`, which is the user token as a `String`.
/// On failure, it returns an `Err` with a `Box<dyn std::error::Error>`.
///
/// # Example
///
/// ```
/// let nks_address = Url::parse("https://nks.example.com").unwrap();
/// let runtime = Runtime::new().unwrap();
/// match runtime.block_on(get_token(nks_address)) {
///     Ok(token) => println!("User token: {}", token),
///     Err(err) => println!("Failed to get token: {}", err),
/// }
/// ```
async fn get_token(nks_address: Url) -> anyhow::Result<String, Box<dyn std::error::Error>> {
    let api_url = nks_address.join("getToken");
    let response: Value = reqwest::Client::new()
        .get(api_url.unwrap())
        .header("accept", "*/*")
        .send()
        .await?
        .json()
        .await?;

    if let Some(user_token) = response.get("token") {
        if let Some(user_token_str) = user_token.as_str() {
            return Ok(user_token_str.to_string())
        }
    }
    println!("The response does not contain a 'token' field");
    Ok(String::new())
}

/// Generates a new key pair and saves it in the NksProvider.
///
/// This asynchronous function sends a POST request to the NksProvider's `generateAndSaveKeyPair` endpoint.
/// It then parses the JSON response and retrieves the `data` field which contains the generated key pair and the `newToken` field which contains the updated token.
///
/// # Arguments
///
/// * `token` - A string slice that represents the user token.
/// * `key_name` - A string slice that represents the name of the key to be generated.
/// * `key_type` - A string slice that represents the type of the key to be generated. The accepted values are "rsa", "ecdsa" and "ecdh".
/// * `nks_address` - A `Url` that specifies the address of the NksProvider.
///
/// # Returns
///
/// A `Result` that, on success, contains an `Ok((String, String))`, where the first string is the JSON representation of the generated key pair and the second string is the updated token.
/// On failure, it returns an `Err` with a `Box<dyn std::error::Error>`.
///
/// # Example
///
/// ```
/// let nks_address = Url::parse("https://nks.example.com").unwrap();
/// let runtime = Runtime::new().unwrap();
/// match runtime.block_on(get_and_save_key_pair("user_token", "key_name", "rsa", nks_address)) {
///     Ok((key_pair, new_token)) => {
///         println!("Key pair: {}", key_pair);
///         println!("New token: {}", new_token);
///     },
///     Err(err) => println!("Failed to generate and save key pair: {}", err),
/// }
/// ```
async fn get_and_save_key_pair(
    token: &str,
    key_name: &str,
    key_type: &str,
    nks_address: Url,
    ) -> Result<(String, String), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let request_body = json!(
            {
            "token": token,
            "name": key_name,
            "type": key_type
            }
        );
    println!("body: {}", request_body);
    let api_url = nks_address.join("generateAndSaveKeyPair");
    let response = client
        .post(api_url.unwrap())
        .header("accept", "*/*")
        .header("Content-Type", "application/json-patch+json")
        .json(&request_body)
        .send()
        .await?;

    let status = response.status(); // Clone the status here
    let response_text = response.text().await?;
    if !status.is_success() {
        let response_json: Value = serde_json::from_str(&response_text)?;
        if let Some(message) = response_json.get("message") {
            if let Some(new_token) = response_json.get("newToken") {
                let token_data = json!({
                    "user_token": new_token.as_str().unwrap()
                });
                fs::write("token.json", token_data.to_string()).expect("Error writing to token.json");
                return Err(format!("Server returned status code: {}. Message: {}", status, message.as_str().unwrap()).into());
            }
        }
        else {
            return Err(format!("Server returned status code: {}", status).into());
        }
    }

    println!("Success response:\n{}", response_text);
    let response_json: Value = serde_json::from_str(&response_text)?;

    // Extract the data field from the response
    let data = response_json.get("data").ok_or_else(|| "Data field not found in the response")?;

    // Convert the data field back to a string
    let data_str = serde_json::to_string_pretty(data)?;

    //save new token
    let user_token = response_json.get("newToken").unwrap().as_str().unwrap().to_string();

    Ok((data_str, user_token))
}

/// Retrieves the secrets from the NksProvider.
///
/// This asynchronous function sends a POST request to the NksProvider's `getSecrets` endpoint.
/// It then parses the JSON response and retrieves the `data` field which contains the secrets and the `newToken` field which contains the updated token.
///
/// # Arguments
///
/// * `token` - A string slice that represents the user token.
/// * `nks_address_str` - A string slice that specifies the address of the NksProvider.
///
/// # Returns
///
/// A `Result` that, on success, contains an `Ok((String, String))`, where the first string is the JSON representation of the secrets and the second string is the updated token.
/// On failure, it returns an `Err` with a `Box<dyn std::error::Error>`.
///
/// # Example
///
/// ```
/// let nks_address_str = "https://nks.example.com";
/// let runtime = Runtime::new().unwrap();
/// match runtime.block_on(get_secrets("user_token", nks_address_str)) {
///     Ok((secrets, new_token)) => {
///         println!("Secrets: {}", secrets);
///         println!("New token: {}", new_token);
///     },
///     Err(err) => println!("Failed to get secrets: {}", err),
/// }
/// ```
async fn get_secrets(token: &str, nks_address_str: &str) -> anyhow::Result<(String, String), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let body = json!({
        "token": token
    });

    let response: Value = client.post(format!("{}getSecrets", nks_address_str))
        .header("accept", "*/*")
        .header("Content-Type", "application/json-patch+json")
        .json(&body)
        .send()
        .await?
        .json()
        .await?;

    //save new token
    let user_token = response.get("newToken").unwrap().as_str().unwrap().to_string();

    // Extract the data field from the response
    let data = response.get("data").ok_or_else(|| "Data field not found in the response")?;

    // Convert the data field back to a string
    let data_str = serde_json::to_string_pretty(data)?;

    Ok((data_str, user_token))
}
