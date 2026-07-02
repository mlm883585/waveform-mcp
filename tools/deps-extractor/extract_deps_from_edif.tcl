# ============================================================================
# extract_deps_from_edif.tcl - 从综合后的 EDIF 网表提取依赖图
# ============================================================================

set argv_list $argv
set params [dict create \
    -project "" \
    -top "" \
    -output "deps_raw.json" \
    -depth 2 \
    -include_internals true \
    -filter_modules "none" \
    -max_cells 4000 \
    -max_nets 20000 \
    -enable_global_clock_fallback false \
]

set i 0
while {$i < [llength $argv_list]} {
    set arg [lindex $argv_list $i]
    if {![dict exists $params $arg]} {
        puts "Warning: unknown argument: $arg"
        incr i
        continue
    }
    set next [expr {$i + 1}]
    if {$next >= [llength $argv_list]} {
        puts "Error: missing value for $arg"
        exit 1
    }
    dict set params $arg [lindex $argv_list $next]
    incr i 2
}

set project_path [dict get $params -project]
set top_module [dict get $params -top]
set output_path [dict get $params -output]

puts "Opening project: $project_path"
if {[catch {open_project $project_path} err]} {
    puts "Error: failed to open project: $err"
    exit 1
}

# 运行综合（如果需要）
puts "Checking synthesis status..."
set synth_status [get_runs synth_1]
if {[get_property STATUS $synth_status] ne "synth_design Complete!"} {
    puts "Launching synthesis..."
    reset_run synth_1
    launch_runs synth_1 -jobs 4
    wait_on_run synth_1
}

# 读取 EDIF 网表
set edif_file "E:/dev/demo21/interface_acu2ant.prj/interface_acu2ant.runs/synth_1/interface_acu2ant.edif"
puts "Reading EDIF netlist: $edif_file"

if {[file exists $edif_file]} {
    read_edif $edif_file
    link_design -top $top_module -part xc7k325tffg900-2
    
    # 验证设计
    set ports [get_ports -quiet]
    puts "DEBUG: Found [llength $ports] ports after link_design"
    
    set cells [get_cells -quiet -hierarchical]
    puts "DEBUG: Found [llength $cells] cells after link_design"
} else {
    puts "Warning: EDIF file not found, trying open_run..."
    open_run synth_1
    link_design -top $top_module -part xc7k325tffg900-2
    
    set ports [get_ports -quiet]
    puts "DEBUG: Found [llength $ports] ports after open_run"
}

# 加载依赖图提取脚本
set script_dir [file dirname [file normalize [info script]]]
source [file join $script_dir "extract_deps.tcl"]

puts "Starting dependency extraction..."

set status [catch {
    extract_deps \
        -top $top_module \
        -output $output_path \
        -depth [dict get $params -depth] \
        -include_internals [dict get $params -include_internals] \
        -filter_modules [dict get $params -filter_modules] \
        -max_cells [dict get $params -max_cells] \
        -max_nets [dict get $params -max_nets] \
        -enable_global_clock_fallback [dict get $params -enable_global_clock_fallback]
} err]

catch {close_project}

if {$status != 0} {
    puts "Error: dependency extraction failed: $err"
    exit 1
}

puts "Dependency extraction complete."
exit 0
