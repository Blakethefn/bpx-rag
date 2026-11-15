use anyhow::Result;

use crate::cli::ModelsAction;

pub(crate) fn run_models(action: ModelsAction) -> Result<()> {
    let config = crate::config::Config::load()?;
    match action {
        ModelsAction::Status => println!("{}", crate::app::embedding_model_status(&config)),
        ModelsAction::Download => {
            let model_dir = crate::app::model_directory(&config);
            crate::embed::Embedder::new_with_device(
                &model_dir,
                &config.embeddings.model,
                config.embeddings.batch_size,
                config.embeddings.intra_threads,
                config.embeddings.device,
            )?;
            println!(
                "Downloaded and loaded {} at {}.",
                config.embeddings.model,
                model_dir.display()
            );
        }
    }
    Ok(())
}
