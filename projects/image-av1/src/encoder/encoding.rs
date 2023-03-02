use std::io::Write;

use image::{
    error::{ParameterError, ParameterErrorKind},
    imageops::FilterType,
    DynamicImage, GenericImage, ImageError, Rgba, Rgba32FImage, RgbaImage,
};
use rav1e::prelude::*;

use crate::{utils::Result, Av1Encoder};

impl Av1Encoder {
    pub fn resize_image(&mut self, image: DynamicImage, filter: FilterType) -> RgbaImage {
        image.resize_exact(self.config.width as u32, self.config.height as u32, filter).to_rgba8()
    }

    pub fn encode_image<F, I>(&mut self, encoder: F, image: &I) -> Result<usize>
    where
        I: GenericImage,
        F: Fn(&I, &mut Frame<u8>),
    {
        self.check_size(image)?;
        let mut size = 0;
        let mut ctx = self.encode_context()?;
        let mut frame = ctx.new_frame();
        encoder(image, &mut frame)?;
        ctx.send_frame(frame).unwrap();
        ctx.flush();
        loop {
            match ctx.receive_packet() {
                Ok(packet) => {
                    size = self.output.write(&packet.data)?;
                    continue;
                }
                Err(EncoderStatus::Encoded) => continue,
                Err(EncoderStatus::LimitReached) => break,
                Err(err) => Err(err).unwrap(),
            }
        }
        Ok(size)
    }
    pub fn write_image_repeats(&mut self, image: RgbaImage, count: usize) -> Result<usize> {
        let mut ctx = self.encode_context()?;
        let frame = ctx.new_frame();
        let mut size = 0;
        for _ in 0..count {
            ctx.send_frame(frame.clone()).unwrap();
        }
        ctx.flush();
        loop {
            match ctx.receive_packet() {
                Ok(packet) => {
                    size = self.output.write(&packet.data)?;
                    continue;
                }
                Err(EncoderStatus::Encoded) => continue,
                Err(EncoderStatus::LimitReached) => break,
                Err(err) => Err(err).unwrap(),
            }
        }
        Ok(size)
    }
    pub fn check_size<I>(&self, image: &I) -> Result<()>
    where
        I: GenericImage,
    {
        if self.config.width != image.width() as usize {
            Err(ImageError::Parameter(ParameterError::from_kind(ParameterErrorKind::DimensionMismatch)))?
        }
        if self.config.height != image.height() as usize {
            Err(ImageError::Parameter(ParameterError::from_kind(ParameterErrorKind::DimensionMismatch)))?
        }
        Ok(())
    }

    fn encode_context(&self) -> Result<Context<u8>> {
        if self.config.chroma_sampling != ChromaSampling::Cs444 {
            panic!("Only 444 chroma sampling is supported")
        }
        let config = Config::new().with_encoder_config(self.config.clone()).with_rate_control(self.rate_control.clone());
        match config.new_context::<u8>() {
            Ok(o) => Ok(o),
            Err(e) => {
                panic!("Error creating context: {}", e)
            }
        }
    }
    fn build_rgba8_frame(&self, image: &RgbaImage, frame: &mut Frame<u8>) -> Result<()> {
        let width = self.config.width;
        let height = self.config.height;
        let mut f = frame.planes.iter_mut();
        let mut planes = image.pixels();

        // it doesn't seem to be necessary to fill padding area
        let mut y = f.next().unwrap().mut_slice(Default::default());
        let mut u = f.next().unwrap().mut_slice(Default::default());
        let mut v = f.next().unwrap().mut_slice(Default::default());

        for ((y, u), v) in y.rows_iter_mut().zip(u.rows_iter_mut()).zip(v.rows_iter_mut()).take(height) {
            let y = &mut y[..width];
            let u = &mut u[..width];
            let v = &mut v[..width];
            for ((y, u), v) in y.iter_mut().zip(u).zip(v) {
                let px = planes.next().expect("Too few pixels");
                let px = rgba8_to_yuv(px);
                *y = px[0];
                *u = px[1];
                *v = px[2];
            }
        }
        Ok(())
    }
    fn build_rgba32_frame(&self, image: &Rgba32FImage, frame: &mut Frame<u8>) -> Result<()> {
        let width = self.config.width;
        let height = self.config.height;
        let mut f = frame.planes.iter_mut();
        let mut planes = image.pixels();

        // it doesn't seem to be necessary to fill padding area
        let mut y = f.next().unwrap().mut_slice(Default::default());
        let mut u = f.next().unwrap().mut_slice(Default::default());
        let mut v = f.next().unwrap().mut_slice(Default::default());

        for ((y, u), v) in y.rows_iter_mut().zip(u.rows_iter_mut()).zip(v.rows_iter_mut()).take(height) {
            let y = &mut y[..width];
            let u = &mut u[..width];
            let v = &mut v[..width];
            for ((y, u), v) in y.iter_mut().zip(u).zip(v) {
                let px = planes.next().expect("Too few pixels");
                let px = rgba32_to_yuv(px);
                *y = px[0];
                *u = px[1];
                *v = px[2];
            }
        }
        Ok(())
    }
    fn init_frame_1(width: usize, height: usize, planes: &[u8], frame: &mut Frame<u8>) -> Result<()> {
        let mut y = frame.planes[0].mut_slice(Default::default());
        let mut planes = planes.into_iter();
        for y in y.rows_iter_mut().take(height) {
            let y = &mut y[..width];
            for y in y.iter_mut() {
                *y = *planes.next().expect("Too few pixels");
            }
        }
        Ok(())
    }
}

// ## RGB to YUV
// Y = (( 66 * R + 129 * G +  25 * B + 128) >> 8) +  16
// U = ((-38 * R -  74 * G + 112 * B + 128) >> 8) + 128
// V = ((112 * R -  94 * G -  18 * B + 128) >> 8) + 128
fn rgba8_to_yuv(rgba: &Rgba<u8>) -> [u8; 3] {
    let r = rgba[0] as i32;
    let g = rgba[1] as i32;
    let b = rgba[2] as i32;
    let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
    [y as u8, u as u8, v as u8]
}

fn rgba32_to_yuv(rgba: &Rgba<f32>) -> [u8; 3] {
    let r = rgba[0];
    let g = rgba[1];
    let b = rgba[2];
    let y = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
    let u = (128.0 - 0.168736 * r - 0.331264 * g + 0.5 * b) as u8;
    let v = (128.0 + 0.5 * r - 0.418688 * g - 0.081312 * b) as u8;
    [y, u, v]
}
