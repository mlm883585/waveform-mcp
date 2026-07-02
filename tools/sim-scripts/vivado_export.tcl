# Vivado Tcl: Export compile order and prepare simulation libraries
# Usage: vivado -mode batch -source vivado_export.tcl
#
# This script:
# 1. Opens the Vivado project
# 2. Exports compile order (filelist.f) for ModelSim
# 3. Prepares simulation libraries (compile_simlib)
# 4. Exports IP outputs if needed

# --- Project path (override via -project argument) ---
# Default: look for project in current directory
set project_file "my_project.xpr"
set output_dir "sim"

# --- Open project ---
open_project $project_file

# --- Export compile order ---
# Generate filelist.f for ModelSim compilation
set filelist_file "${output_dir}/filelist.f"
set fp [open $filelist_file w]

# Get all source files from the project
set sources [get_files -of_objects [get_filesets sources_1]]
foreach src $sources {
    # Only include Verilog/SystemVerilog files
    set ext [file extension $src]
    if {$ext == ".v" || $ext == ".sv" || $ext == ".vh" || $ext == ".svh"} {
        puts $fp $src
    }
}

# Get testbench files
set tb_sources [get_files -of_objects [get_filesets sim_1]]
foreach src $tb_sources {
    set ext [file extension $src]
    if {$ext == ".v" || $ext == ".sv" || $ext == ".vh" || $ext == ".svh"} {
        puts $fp $src
    }
}

close $fp
puts "INFO: Compile order exported to $filelist_file"

# --- Prepare simulation libraries ---
# compile_simlib creates ModelSim-compatible libraries for Xilinx IP
set simlib_dir "D:/eda_libs/vivado_2018_3_modelsim"
compile_simlib -simulator modelsim -library unisims -directory $simlib_dir
puts "INFO: Simulation libraries prepared in $simlib_dir"

# --- Close project ---
close_project

puts "INFO: Vivado export complete"