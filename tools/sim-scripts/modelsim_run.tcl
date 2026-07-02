# ModelSim Tcl: Compile, simulate, dump waveform, collect assertions, exit
# Usage: vsim -c -do "source modelsim_run.tcl" -t 1ps work.tb_top
#
# This script is called by run_sim_modelsim.ps1.
# Customize the wave dump scope and assertion settings per project.

# --- Transcript setup ---
transcript file sim_output/transcript.log

# --- Compile (done by PowerShell calling vlog separately) ---
# This script assumes compilation is already done by the outer PowerShell script.
# If you need to compile here, uncomment:
# vlog -sv +acc +define+SIM -work sim_work [filelist contents]

# --- Simulation ---
vsim -assertdebug work.tb_top

# --- Waveform dump configuration ---
log -recursive /tb_top/dut/*

# Add critical signals explicitly
add wave /tb_top/dut/data_o
add wave /tb_top/dut/valid
add wave /tb_top/dut/clk
add wave /tb_top/dut/reset

# --- Run simulation ---
# Mode: run_all (run until $finish or assertion failure)
run -all

# --- Assertion statistics ---
# ModelSim reports assertion counts in transcript.
# The PowerShell script parses transcript for this information.

# --- Waveform export ---
# VCD export (primary format for wellen compatibility)
vcd file sim_output/dump.vcd
vcd add -recursive /tb_top/dut/*
vcd flush

# --- Summary ---
quit -f