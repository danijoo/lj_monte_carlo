mod trajectory;
use trajectory::*;
mod energy;
use energy::*;
use std::env;

const LJ_EPS : f64 = 1.0;
const LJ_SIG : f64 = 1.0;


fn get_virial(distance: f64) -> f64 {
    let r7 = (LJ_SIG/distance).powi(7);
    let r13 = (LJ_SIG/distance).powi(13);
    return 24.0 * LJ_EPS / LJ_SIG * ( r7-2.0*r13 );
}

#[test]
fn test_get_viral() {
    let result = get_virial(2.5);
    let expected = 0.038999477;
    assert!( (result - expected) < 0.0001, "{}", result );
}

fn get_distance_with_pbc(x1: f64, x2: f64, length: f64, half_length: f64) -> f64 {
    let mut d = (x1-x2).abs();
    if d > half_length { d -= length }
    else if d < -half_length { d += length }
    return d;
}

fn eval_surface_tension(box_z: f64, p_zz: f64, p_xy: f64) -> f64 {
    return box_z / 2.0 * (p_zz - p_xy);
}

#[test]
fn test_eval_surface_tension() {
    let expected = 2.0;
    let result = eval_surface_tension(2.0,5.0,3.0);
    assert!( (result-expected).abs() < 0.0001, "{}", result );
}


fn main() {

    // parse args
    let args: Vec<String> = env::args().collect();
    let mut filename = "montecarlo.xyz".to_string();
    let mut skip: usize = 0;
    for i in 0..args.len() {
        if args[i] == "-f" {
            filename = args[i + 1].clone();
        } else if args[i] == "-s" {
            skip = args[i + 1].parse::<usize>().unwrap();
        }
    }

    // open file and skip to requiested position
    let mut trj_reader = TrjReader::new(&filename);
    if skip > 0 { trj_reader.skip(skip) };

    // trajectory information
    let mut frame = trj_reader.next_frame();
    println!("{:?}", frame);

    let volume = frame.box_x * frame.box_y * frame.box_z;
    let density = frame.num_particles as f64 / volume;
    let num_particles = frame.num_particles;

    let box_half_x = frame.box_x / 2.0;
    let box_half_y = frame.box_y / 2.0;
    let box_half_z = frame.box_z / 2.0;

    let mut frame_count = 0;
    let mut p_xy_sum = 0.0;
    let mut p_z_sum = 0.0;

    let variable_without_name = frame.temperature/LJ_EPS * density;

    println!("~~~ THIS IS A RUNNING AVERAGE! ~~~");
    loop {
        frame_count += 1;

        let mut trace_xy = 0.0;
        let mut trace_z = 0.0;
        for i in 0..num_particles {
            for j in i+1..num_particles {
                let dist_sqrt = get_particle_distance_squared(frame.rx[i], frame.ry[i], frame.rz[i], frame.rx[j], frame.ry[j], frame.rz[j], frame.box_x, frame.box_y, frame.box_z, box_half_x, box_half_y, box_half_z);
                let dist = dist_sqrt.sqrt();
                let dx = get_distance_with_pbc(frame.rx[i], frame.rx[j], frame.box_x, box_half_x);
                let dy = get_distance_with_pbc(frame.ry[i], frame.ry[j], frame.box_y, box_half_y);
                let dz = get_distance_with_pbc(frame.rz[i], frame.rz[j], frame.box_z, box_half_z);
                let virial = get_virial(dist);
                trace_xy += (dx * dx + dy * dy) / dist * virial;
                trace_z += (dz * dz) / dist * virial;
            }
        }
        let p_xy = variable_without_name - 1.0/(2.0*volume)*(trace_xy);
        let p_zz = variable_without_name - 1.0/volume*(trace_z);

        p_xy_sum += p_xy;
        p_z_sum += p_zz;

        ///////////////////////////////////
        if frame_count % 10 == 0 {
            let p_z_avg = p_z_sum / frame_count as f64;
            let p_xy_avg = p_xy_sum / frame_count as f64;
            let p_diff = p_z_avg - p_xy_avg;
            let surface_tension = eval_surface_tension(frame.box_z, p_z_avg, p_xy_avg);
            println!("Frame {}\t\tzz: {:.5}\txy: {:.5}\tdifference: {:.5}\t\ttension: {:.5}", frame_count, p_z_avg, p_xy_avg, p_diff, surface_tension);

            // frame_count = 0;
            // p_z_sum = 0.0;
            // p_xy_sum = 0.0;
        }

        if !trj_reader.update_with_next(&mut frame) { break }
    }

}
