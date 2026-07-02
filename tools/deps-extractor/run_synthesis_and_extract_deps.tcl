# ============================================================================
# run_synthesis_and_extract_deps.tcl - 综合后提取依赖图
# ============================================================================
# 用法:
#   vivado -mode batch -source run_synthesis_and_extract_deps.tcl -tclargs \
#       -project <path_to_xpr> \
#       -top <top_module> \
#       -output <deps_raw.json>
# ============================================================================

# 解析参数
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

if {[dict get $params -top] eq ""} {
    puts "Error: -top is required"
    exit 1
}

set project_path [dict get $params -project]
set top_module [dict get $params -top]
set output_path [dict get $params -output]

# 加载依赖图提取脚本
set script_dir [file dirname [file normalize [info script]]]
source [file join $script_dir "extract_deps.tcl"]

# 打开工程
puts "Opening project: $project_path"
if {[catch {open_project $project_path} err]} {
    puts "Error: failed to open project: $err"
    exit 1
}

# 运行综合（如果未运行过）
puts "Checking synthesis status..."
set synth_status [get_runs synth_1]
if {[get_property STATUS $synth_status] eq "synth_design Complete!"} {
    puts "Synthesis already complete, opening run..."
    open_run synth_1
} else {
    puts "Launching synthesis..."
    reset_run synth_1
    launch_runs synth_1 -jobs 4
    wait_on_run synth_1
    open_run synth_1
}

# 设置当前设计（关键步骤！）
puts "Opening synthesis run..."
open_run synth_1

puts "Synthesis complete. Starting dependency extraction..."

# 验证设计是否加载
set ports [get_ports -quiet]
puts "DEBUG: Found [llength $ports] top-level ports"
if {[llength $ports] > 0} {
    puts "DEBUG: First 5 ports: [lrange $ports 0 4]"
}

# 提取依赖图
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

# 关闭工程
catch {close_project}

if {$status != 0} {
    puts "Error: dependency extraction failed: $err"
    exit 1
}

puts "Synthesis and dependency extraction complete."
exit 0
