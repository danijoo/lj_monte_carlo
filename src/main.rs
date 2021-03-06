#![allow(non_snake_case)]

extern crate rand;
use rand::Rng;
use rand::distributions::{IndependentSample, Range};
mod energy;
use energy::*;
use std::io::prelude::*;
extern crate argparse;
use argparse::{ArgumentParser, Store, StoreFalse, StoreTrue};
mod trajectory;
use trajectory::*;

// LJ params
const LJ_EPS : f64 = 1.0;
const LJ_SIG : f64 = 1.0;

 // intended acceptance rate = 33%
const TRIES_INTENDED : f64 = 3.0;

// factor for auto displacement scaling
const DISP_SCALE_FACTOR : f64 = 0.1;
const SCALE_INTERVAL : usize = 5000;

const EQUILIBRATION_OUTPUT_INTERVAL : usize = 5000;
const SAMPLING_OUTPUT_INTERVAL : usize = 5000;

// easy printing to stderr
macro_rules! println_stderr(
    ($($arg:tt)*) => { {
        let r = writeln!(&mut ::std::io::stderr(), $($arg)*);
        r.expect("failed printing to stderr");
    } }
);

fn main() {
    println_stderr!("");
    println_stderr!("################################################################");
    println_stderr!("##################  LJ Monte Carlo Simulation  #################");
    println_stderr!("################################################################");
    println_stderr!("");

    /** Definition of default run parameters **/
    let mut eq_steps  = 1000000;
    let mut sample_steps = 100000;

    let mut num_particles: usize = 512;
    let mut density = 0.7;
    let mut temperature = 0.9;

    let mut cutoff = 3.0;

    let mut TAILCORR : bool = true;
    let mut SHIFT: bool = true;

    let mut displacement = 0.1; // max particle displacement in one dimension
    let mut SCALE: bool = true; // switch for displacement scaling

    // scale factor in z for vaccuum space
    let mut vacuum_slab = 0.0;

    // output config
    let mut output_prefix = "montecarlo".to_string(); // .xyz will be append
    let mut output_interval : i64 = 100;
    let mut output_minim : bool = false;

    // parse cmd line arguments and override defaults
    parse_cmd_args(&mut sample_steps, &mut eq_steps, &mut num_particles,
                   &mut density, &mut temperature,
                   &mut cutoff, &mut displacement, &mut SCALE, &mut TAILCORR, &mut SHIFT,
                   &mut output_prefix, &mut output_interval, &mut output_minim,
                   &mut vacuum_slab);


    /** Initialize the system **/
    let beta = 1.0/temperature;

    let mut volume = (num_particles as f64)/ density;
    let length  = volume.cbrt();
    let (l_x, l_y, mut l_z) = (length, length, length);

    let cutoff_squared = cutoff * cutoff;
    let max_displacement = length / 2.0; // displacement wont be scaled over that

    // initialize randomness - TODO seed?
    let mut rng = rand::thread_rng();
    let particle_range = Range::new(0, num_particles);

    // randomly place particles in the box
    let mut rx : Vec<f64> = vec![];
    let mut ry : Vec<f64> = vec![];
    let mut rz : Vec<f64> = vec![];
    loop {
        rx.push(l_x * rng.gen::<f64>());
        ry.push(l_y * rng.gen::<f64>());
        rz.push(l_z * rng.gen::<f64>());
        if rx.len() == num_particles { break; }
    }

    // scale box in z for vacuum space and move particles in the middle of the box
    if vacuum_slab > 0.0 {
        let scale = vacuum_slab + 1.0;
        l_z *= scale;
        volume *= scale;
        density /= scale;
        let move_z = l_z/scale*vacuum_slab/2.0;
        for i in 0..num_particles {
            rz[i] += move_z;
        }
    }

    // calculation of shift and tailcorrections
    let e_shift = if SHIFT { 4.0 * LJ_EPS * ( (LJ_SIG/cutoff).powi(12) - (LJ_SIG/cutoff).powi(6) ) } else { 0.0 };
    let e_corr = if TAILCORR { 8.0/3.0*std::f64::consts::PI*density*LJ_EPS*LJ_SIG.powi(3)*((1.0/3.0*(LJ_SIG/cutoff).powi(9)) - (LJ_SIG/cutoff).powi(3)) } else { 0.0 };
    let p_corr = if TAILCORR { 16.0/3.0*std::f64::consts::PI*density.powi(2)*LJ_EPS*LJ_SIG.powi(3)*((2.0/3.0*(LJ_SIG/cutoff).powi(9)) - (LJ_SIG/cutoff).powi(3)) } else { 0.0 };

    println_stderr!("Particles: {}, Density: {}, Temperature: {}", num_particles, density, temperature);
    println_stderr!("System volume: {:8.3}, Dimensions {:.3}/{:.3}/{:.3}", volume, l_x, l_y, l_z);
    println_stderr!("Minimization steps: {}, Sampling steps: {}", eq_steps, sample_steps);
    println_stderr!("LJ params eps: {}, sigma: {}, cutoff: {}", LJ_EPS, LJ_SIG, cutoff);
    println_stderr!("Tailcorr: {:8.3}, Shift: {:8.3}, Pressurecorr: {:8.3}", e_corr, e_shift, p_corr);

    // energy and average sums
    let (mut energy, mut virial) = get_total_energy(&rx, &ry, &rz, num_particles, l_x, l_y, l_z, cutoff_squared, e_corr, e_shift);
    let mut energy_sum = 0.0;
    let mut virial_sum = 0.0;
    let mut step_counter = 0;
    let mut accept_counter = 0;


    // prepare and write first trajectory frame
    let mut trajectory : XYZTrajectory = XYZTrajectory::new(&format!("{}.xyz", output_prefix));
    if output_minim { trajectory.write(&rx, &ry, &rz, num_particles, l_x, l_y, l_z, temperature, LJ_EPS, LJ_SIG, cutoff, true); }


    println_stderr!("");
    println_stderr!("################################################################");
    println_stderr!("########################  Equilibration  #######################");
    println_stderr!("################################################################");
    println_stderr!("");


    // START OF METROPOLIS
    /*****************************************************************************************/

    for step in 0..eq_steps+sample_steps {

        // select rnd particle
        let rnd_index = particle_range.ind_sample(&mut rng);

        // store old position
        let oldX = rx[rnd_index];
        let oldY = ry[rnd_index];
        let oldZ = rz[rnd_index];

        // old particle energy
        let (old_particle_energy, old_particle_virial) = get_particle_energy(&rx, &ry, &rz, rnd_index, num_particles, l_x, l_y, l_z, cutoff_squared, e_shift);

        // rnd displacement and PBC
        rx[rnd_index] += ( rng.gen::<f64>() - 0.5 ) * displacement;
        ry[rnd_index] += ( rng.gen::<f64>() - 0.5 ) * displacement;
        rz[rnd_index] += ( rng.gen::<f64>() - 0.5 ) * displacement;
        if rx[rnd_index] < 0.0 { rx[rnd_index] += l_x }
        if rx[rnd_index] >= l_x { rx[rnd_index] -= l_x }
        if ry[rnd_index] < 0.0 { ry[rnd_index] += l_y }
        if ry[rnd_index] >= l_y { ry[rnd_index] -= l_y }
        if rz[rnd_index] < 0.0 { rz[rnd_index] += l_z }
        if rz[rnd_index] >= l_z { rz[rnd_index] -= l_z }

        // calculate energy difference
        let (new_particle_energy, new_particle_virial) = get_particle_energy(&rx, &ry, &rz, rnd_index, num_particles, l_x, l_y, l_z, cutoff_squared, e_shift);

        let dE = new_particle_energy - old_particle_energy;

        // acceptance rule
        if dE < 0.0 || rng.gen::<f64>() < (-beta * dE).exp() {
            accept_counter += 1;
            energy += dE;
            virial += new_particle_virial - old_particle_virial;

            // recalculate total energy every 1000 steps to account for rounding errors in particle energy function
            if step % 10000 == 0 {
                let (e, v) = get_total_energy(&rx, &ry, &rz, num_particles, l_x, l_y, l_z, cutoff_squared, e_corr, e_shift);
                energy = e;
                virial = v;
            }
        } else {
            // restore old positions if move is rejected
            rx[rnd_index] = oldX;
            ry[rnd_index] = oldY;
            rz[rnd_index] = oldZ;
        }

        // update average sums
        step_counter += 1;
        energy_sum += energy;
        virial_sum += virial;

        // reset average sums for sampling
        if step == eq_steps {
            println_stderr!("");
            println_stderr!("################################################################");
            println_stderr!("##########################  Sampling  ##########################");
            println_stderr!("################################################################");
            println_stderr!("");
            step_counter = 0;
            accept_counter = 0;
            energy_sum = 0.0;
            virial_sum = 0.0;
        }

        // Everything below here is not part of the metropolis sampling (extras)

        // print some output during equilibration
        if step < eq_steps && step_counter % EQUILIBRATION_OUTPUT_INTERVAL == 0 && step != 0 {
            let tries_per_step : f64 = step_counter as f64 /accept_counter as f64;
            let acceptance_rate = 1.0/tries_per_step * 100.0;
            let avg_energy = energy_sum / step_counter as f64;
            let avg_virial = virial_sum / step_counter as f64;
            println_stderr!("Eq {:<10} Energy: {:<30.3} Virial: {:<30.3} Accept.: {:<4.1}%   dr: {:.3}", step, avg_energy, avg_virial, acceptance_rate, displacement);
        }

        // displacement scaling during equilibration for good acceptance ratios
        if SCALE && step < eq_steps && step % SCALE_INTERVAL == 0 {
            let tries_per_step : f64 = step_counter as f64 /accept_counter as f64;

            // will increase the max displacement if the acceptance rate is too high and vice versa
            let scale_factor = (TRIES_INTENDED/tries_per_step * DISP_SCALE_FACTOR).abs();
            if tries_per_step < TRIES_INTENDED - 0.2 && displacement < max_displacement {
                displacement += displacement * scale_factor;
            } else if tries_per_step > TRIES_INTENDED + 0.2 && displacement > 0.0 {
                displacement -= displacement * scale_factor;
            }
            step_counter = 0;
            accept_counter = 0;
            energy_sum = 0.0;
        }

        // print some output during sampling
        if step > eq_steps && step_counter % SAMPLING_OUTPUT_INTERVAL == 0 {
            println_stderr!("Step  {:<10} Energy: {:<30.3}", step_counter, energy);
        }

        // write trajectory
        if step as i64 % output_interval == 0 {
            if step > eq_steps || output_minim {
                trajectory.write(&rx, &ry, &rz, num_particles, l_x, l_y, l_z, temperature, LJ_EPS, LJ_SIG, cutoff, true);
            }
        }
    }

    // END OF METROPOLIS
    /*****************************************************************************************/
    println_stderr!("Done sampling!");

    let final_energy = energy_sum/step_counter as f64;
    let particle_energy = final_energy / num_particles as f64;
    let final_virial = virial_sum / 3.0 / step_counter as f64 / volume;
    let pressure = virial_sum / 3.0 / step_counter as f64 / volume + density * temperature + p_corr;
    let final_acceptance_rate = 1.0/((accept_counter as f64)/(step_counter as f64)) * 100.0;

    println_stderr!("");
    println_stderr!("################################################################");
    println_stderr!("##########################  Results  ###########################");
    println_stderr!("################################################################");
    println_stderr!("");
    println!(
"Minimization: {}
Steps: {}

# Lennard Jones Params
epsilon: {}
sigma: {}
cutoff: {}

# System
Particles: {}
Density: {}
Temperature: {}
Volume: {}
Box dimension: {:.3}/{:.3}/{:.3}
Max Displacement: {}

# Correction
Energy correction: {}
Shift: {}
P-Correction: {}

# Averages
Tries: {}
Accepted: {}
Acceptance: {:.2}%
Energy: {}
Energy per particle: {}
Virial: {}
Pressure: {}",
         eq_steps, sample_steps,
        LJ_EPS, LJ_SIG, cutoff,
        num_particles, density, temperature, volume, l_x, l_y, l_z, displacement,
        e_corr, e_shift, p_corr,
        step_counter, accept_counter, final_acceptance_rate, final_energy, particle_energy, final_virial, pressure);

    trajectory.write(&rx, &ry, &rz, num_particles, l_x, l_y, l_z, temperature, LJ_EPS, LJ_SIG, cutoff, true);
}

// Parse command line arguments
fn parse_cmd_args(NUM_STEPS: &mut usize, NUM_eq_steps: &mut usize,
                  NUM_PARTICLES: &mut usize, DENSITY: &mut f64, TEMPERATURE: &mut f64,
                  CUTOFF: &mut f64, MAX_DISP_START: &mut f64, SCALE: &mut bool, TAILCORR: &mut bool, SHIFT: &mut bool,
                  OUTPUT_PREFIX: &mut String, OUTPUT_INTERVAL: &mut i64, OUTPUT_MINIM: &mut bool,
                  VACUUM_SLAB: &mut f64) {
    let mut ap = ArgumentParser::new();
    ap.set_description("LJ MC simulation.");
    ap.refer(NUM_STEPS)
        .add_option(&["-n", "--nsteps"], Store,
                    "Simulation steps: Number of steps for averaging" );
    ap.refer(NUM_eq_steps)
        .add_option(&["-m", "--nminimsteps"], Store,
                    "Minimization steps: Number of steps before averaging starts");
    ap.refer(NUM_PARTICLES)
        .add_option(&["-p", "--nparticles"], Store,
                    "Total number of particles");
    ap.refer(DENSITY)
        .add_option(&["-d", "--density"], Store,
                    "Particle density");
    ap.refer(TEMPERATURE)
        .add_option(&["-t", "--temperature"], Store,
                    "Temperature");
    ap.refer(CUTOFF)
        .add_option(&["--cutoff"], Store,
                    "Lennard jones cutoff radius in length of epsilon");
    ap.refer(MAX_DISP_START)
        .add_option(&["--displacement"], Store,
                    "Displacement per trial move");
    ap.refer(SCALE)
        .add_option(&["--nodisplacementscale"], StoreFalse,
                    "Disable displacement scaling");
    ap.refer(OUTPUT_PREFIX)
        .add_option(&["-o", "--output"], Store,
                    "Output file prefix");
    ap.refer(OUTPUT_INTERVAL)
        .add_option(&["--osteps"], Store,
                    "Number of steps between writing to the trajectory file. -1 only writes last frame");
    ap.refer(OUTPUT_MINIM)
        .add_option(&["--writeminimization"], StoreTrue,
                    "Enables writing of minimization step to trajectory");
    ap.refer(VACUUM_SLAB)
        .add_option(&["--vacuum"], Store,
                    "Dimension of vacuum space above the intial system relative to the rest of the system (0=no slab, 1=half filled system, 2=thrid filled system ...).");
    ap.refer(TAILCORR)
        .add_option(&["--notailcorr"], StoreFalse,
                    "Disable tailcorrection");
    ap.refer(SHIFT)
        .add_option(&["--noshift"], StoreFalse,
                    "Disable lj shifting");
    ap.parse_args_or_exit();
}
