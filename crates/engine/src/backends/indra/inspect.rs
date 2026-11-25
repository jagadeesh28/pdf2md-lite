#[cfg(test)]
mod tests {
    use std::fs::File;
    use tract_onnx::prelude::*;

    #[test]
    fn test_det_with_concrete_shape() -> anyhow::Result<()> {
        let path = crate::resolve_model_path("models/indra/det.onnx");
        let mut file = File::open(&path)?;

        let _model = tract_onnx::onnx()
            .model_for_read(&mut file)?
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, 1024, 1024)),
            )?
            .into_typed()?
            .into_runnable()?;

        Ok(())
    }

    #[test]
    fn test_rec_with_concrete_shape() -> anyhow::Result<()> {
        let path = crate::resolve_model_path("models/indra/rec.onnx");
        let mut file = File::open(&path)?;

        let _model = tract_onnx::onnx()
            .model_for_read(&mut file)?
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, 96, 320)),
            )?
            .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1)))?
            .into_typed()?
            .into_runnable()?;

        Ok(())
    }

    #[test]
    fn test_rec_world_with_concrete_shape() -> anyhow::Result<()> {
        let path = crate::resolve_model_path("models/indra/rec_world.onnx");
        if !path.exists() {
            return Ok(());
        }
        let mut file = File::open(&path)?;

        let _model = tract_onnx::onnx()
            .model_for_read(&mut file)?
            .with_input_fact(
                0,
                InferenceFact::dt_shape(f32::datum_type(), tvec!(1, 1, 32, 320)),
            )?
            .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1)))?
            .into_typed()?
            .into_runnable()?;

        Ok(())
    }
}
