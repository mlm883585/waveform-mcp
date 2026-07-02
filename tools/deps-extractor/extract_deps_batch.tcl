set argv_list $argv
set params [dict create \
    -project "" \
    -top "" \
    -output "deps_raw.json" \
    -depth 2 \
    -include_internals true \
    -filter_modules "" \
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

set script_dir [file dirname [file normalize [info script]]]
source [file join $script_dir "extract_deps.tcl"]

set project_path [dict get $params -project]
set project_opened 0

if {$project_path ne ""} {
    puts "Opening project: $project_path"
    if {[catch {open_project $project_path} err]} {
        puts "Error: failed to open project: $err"
        exit 1
    }
    set project_opened 1
}

set status [catch {
    puts "Starting dependency extraction..."
    extract_deps \
        -top [dict get $params -top] \
        -output [dict get $params -output] \
        -depth [dict get $params -depth] \
        -include_internals [dict get $params -include_internals] \
        -filter_modules [dict get $params -filter_modules] \
        -max_cells [dict get $params -max_cells] \
        -max_nets [dict get $params -max_nets] \
        -enable_global_clock_fallback [dict get $params -enable_global_clock_fallback]
} err]

if {$project_opened} {
    catch {close_project}
}

if {$status != 0} {
    puts "Error: dependency extraction failed: $err"
    exit 1
}

puts "Batch extraction complete."
exit 0
