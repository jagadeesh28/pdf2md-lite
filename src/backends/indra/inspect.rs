#[cfg(test)]
mod tests {
    use std::fs::File;
    use tract_onnx::prelude::*;

    #[test]
    fn test_det_with_concrete_shape() -> anyhow::Result<()> {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let path = std::path::Path::new(&manifest_dir).join("models").join("indra").join("det.onnx");
        let mut file = File::open(&path)?;

        let _model = tract_onnx::onnx()
            .model_for_read(&mut file)?
            .with_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, 1024, 1024)))?
            .into_typed()?
            .into_runnable()?;

        println!("Successfully typed and optimized DBNet with concrete shape (1, 1, 1024, 1024)!");
        Ok(())
    }

    #[test]
    fn test_rec_with_concrete_shape() -> anyhow::Result<()> {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let path = std::path::Path::new(&manifest_dir).join("models").join("indra").join("rec.onnx");
        let mut file = File::open(&path)?;

        let _model = tract_onnx::onnx()
            .model_for_read(&mut file)?
            .with_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, 96, 320)))?
            .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1)))?
            .into_typed()?
            .into_runnable()?;

        println!("Successfully typed and optimized SVTR with concrete shape (1, 1, 96, 320)!");
        Ok(())
    }

    #[test]
    fn test_rec_world_with_concrete_shape() -> anyhow::Result<()> {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let path = std::path::Path::new(&manifest_dir).join("models").join("indra").join("rec_world.onnx");
        if !path.exists() {
            return Ok(());
        }
        let mut file = File::open(&path)?;

        let _model = tract_onnx::onnx()
            .model_for_read(&mut file)?
            .with_input_fact(0, InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, 32, 320)))?
            .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1)))?
            .into_typed()?
            .into_runnable()?;

        println!("Successfully typed and optimized SVTR world with concrete shape (1, 1, 32, 320)!");
        Ok(())
    }
}
