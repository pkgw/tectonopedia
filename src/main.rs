// Copyright 2022 the Tectonic Project
// Licensed under the MIT License

use clap::{Args, Parser, Subcommand};
use std::{fs::File, io::Write, time::Instant};
use tectonic::status::termcolor::TermcolorStatusBackend;
use tectonic_errors::prelude::*;
use tectonic_status_base::{tt_note, ChatterLevel, StatusBackend};

mod config;
mod holey_vec;
mod index;
mod inputs;
mod metadata;
mod multivec;
mod pass1;
mod pass2;
#[macro_use]
mod texworker;
mod tex_escape;
mod worker_status;

use worker_status::WorkerStatusBackend;

use string_interner::DefaultSymbol as InputId;

fn main() {
    let args = ToplevelArgs::parse();

    let mut status = match &args.action {
        Action::FirstPassImpl(a) => {
            Box::new(WorkerStatusBackend::new(&a.tex_path)) as Box<dyn StatusBackend>
        }
        Action::SecondPassImpl(a) => {
            Box::new(WorkerStatusBackend::new(&a.tex_path)) as Box<dyn StatusBackend>
        }
        _ => Box::new(TermcolorStatusBackend::new(ChatterLevel::Normal)) as Box<dyn StatusBackend>,
    };

    if let Err(e) = args.exec(status.as_mut()) {
        status.report_error(&e);
        std::process::exit(1)
    }
}

#[derive(Debug, Parser)]
struct ToplevelArgs {
    #[command(subcommand)]
    action: Action,
}

impl ToplevelArgs {
    fn exec(self, status: &mut dyn StatusBackend) -> Result<()> {
        match self.action {
            Action::Build(a) => a.exec(status),
            Action::FirstPassImpl(a) => a.exec(status),
            Action::SecondPassImpl(a) => a.exec(status),
        }
    }
}

#[derive(Debug, Subcommand)]
enum Action {
    Build(BuildArgs),
    FirstPassImpl(pass1::FirstPassImplArgs),
    SecondPassImpl(pass2::SecondPassImplArgs),
}

#[derive(Args, Debug)]
struct BuildArgs {
    #[arg(long)]
    sample: Option<String>,
}

impl BuildArgs {
    fn exec(self, status: &mut dyn StatusBackend) -> Result<()> {
        let t0 = Instant::now();

        // Set up data structures

        let mut indices = index::IndexCollection::default();

        atry!(
            indices.load_user_indices();
            ["failed to load user indices"]
        );

        // First pass of indexing and gathering font/asset information.

        let mut p1r = pass1::Pass1Reducer::new(indices);
        let ninputs = texworker::reduce_inputs(&mut p1r, status)?;
        tt_note!(status, "pass 1: complete - processed {} inputs", ninputs);
        let (assets, indices) = p1r.unpack();

        // Resolve cross-references and validate.

        atry!(
            indices.validate_references();
            ["failed to validate cross-references"]
        );

        // Pass 2, emitting

        let mut entrypoints_file = atry!(
            File::create("build/_all.html");
            ["error creating output `build/_all.html`"]
        );

        // By adding the script reference here, we get Parcel.js to emit the
        // associated built files under this file's name. Otherwise they get
        // tied to whatever happens to be the first entry that we emit.
        atry!(
            writeln!(entrypoints_file, "<script src=\"../web/index.ts\" type=\"module\"></script>");
            ["error writing to output `build/_all.html`"]
        );

        let mut p2r = pass2::Pass2Reducer::new(assets, indices, entrypoints_file);
        texworker::reduce_inputs(&mut p2r, status)?;
        tt_note!(status, "pass 2: complete");

        tt_note!(
            status,
            "build took {:.1} seconds",
            t0.elapsed().as_secs_f32()
        );
        Ok(())
    }
}
