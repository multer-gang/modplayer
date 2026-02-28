pub fn calculate_stm_tempo(tempo: u8) -> u8 {
    // https://gist.github.com/cs127/c0f5dffc9f4db2221a66fd9ffc1e3edb
    let st1_tpr: usize = (tempo >> 4).into(); // upper digit (same as ST3 "speed" / ticks per row)
    let st1_fac: f32 = (tempo & 0xF).into(); // lower digit
    let st_mixing_rate: f32 = 15909.0;
    let factor_constants: [f32; 16] = [140.0, 50.0, 25.0, 15.0, 10.0, 7.0, 6.0, 4.0, 3.0, 3.0, 2.0, 2.0, 2.0, 2.0, 1.0, 1.0];
    let mut samples_per_tick: f32 = st_mixing_rate / (50.0 - ((factor_constants[st1_tpr] * st1_fac) / 16.0));

    if samples_per_tick <= 0.0 {
        samples_per_tick += 65536.0;
    }

    (st_mixing_rate * 5.0 / (samples_per_tick * 2.0)) as u8
}
