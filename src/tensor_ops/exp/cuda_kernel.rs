use crate::tensor_ops::cuda_kernels::cuda_unary;

unsafe impl cudarc::driver::DeviceRepr for super::ExpKernelOp {}

const PTX: &str = include_str!(concat!(env!("OUT_DIR"), "/exp.ptx"));

cuda_unary!(df(f(x)) super::ExpKernelOp, f32, PTX, "exp_fwd_f32", "exp_bwd_f32");
cuda_unary!(df(f(x)) super::ExpKernelOp, f64, PTX, "exp_fwd_f64", "exp_bwd_f64");
