use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let octo = octocrab::Octocrab::builder().build()?;
    for route in ["/rate_limit", "rate_limit"] {
        match octo
            .get::<serde_json::Value, _, _>(route, None::<&[(&str, &str)]>)
            .await
        {
            Ok(v) => println!("route '{route}': ok"),
            Err(e) => println!("route '{route}': FAIL {e}"),
        }
    }
    Ok(())
}
