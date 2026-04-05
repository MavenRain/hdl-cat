//! Trace the 4-bit counter to VCD and print the output.
//!
//! Redirect to a `.vcd` file and open in `GTKWave` / Surfer.

use hdl_cat::prelude::*;
use hdl_cat_sim::trace_to_string;

fn main() -> Result<(), hdl_cat_error::Error> {
    let c = std_lib::counter::<4>()?;
    let inputs: Vec<hdl_cat_kind::BitSeq> =
        (0..8).map(|_| hdl_cat_kind::BitSeq::new()).collect();
    let vcd = trace_to_string(&c, inputs)?;
    print!("{vcd}");
    Ok(())
}
