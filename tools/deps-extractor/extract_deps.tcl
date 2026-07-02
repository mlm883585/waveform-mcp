namespace eval ::deps_extractor {

variable current_stage ""
variable stage_started_ms 0

proc check_vivado_version {} {
    set status [catch {
        set ver [version -short]
        puts "Vivado version: $ver"
        if {[regexp {(\d+)\.(\d+)} $ver _ major minor]} {
            if {$major < 2018 || ($major == 2018 && $minor < 3)} {
                puts "WARNING: Vivado $ver may not be fully supported. Minimum recommended: 2018.3"
            }
        }
    } err]
    if {$status != 0} {
        puts "ERROR: This script must be run inside Vivado Tcl."
        puts "Usage: vivado -mode batch -source extract_deps_batch.tcl -tclargs -top <module>"
        return 0
    }
    return 1
}

proc stage_begin {name} {
    variable current_stage
    variable stage_started_ms
    set current_stage $name
    set stage_started_ms [clock milliseconds]
    puts "==> $name"
}

proc stage_end {summary} {
    variable current_stage
    variable stage_started_ms
    set elapsed [expr {[clock milliseconds] - $stage_started_ms}]
    if {$summary ne ""} {
        puts "<== $current_stage (${elapsed} ms): $summary"
    } else {
        puts "<== $current_stage (${elapsed} ms)"
    }
}

proc to_bool {value default_value} {
    if {$value eq ""} {
        return $default_value
    }
    set normalized [string tolower $value]
    if {$normalized in {"1" "true" "yes" "on"}} {
        return 1
    }
    if {$normalized in {"0" "false" "no" "off"}} {
        return 0
    }
    return $default_value
}

proc trim_list {items max_count label} {
    if {$max_count <= 0} {
        return $items
    }
    set item_count [llength $items]
    if {$item_count <= $max_count} {
        return $items
    }
    puts "INFO: limiting $label from $item_count to $max_count"
    return [lrange $items 0 [expr {$max_count - 1}]]
}

proc pin_leaf_name {pin} {
    set pin_name ""
    catch {set pin_name [get_property REF_PIN_NAME $pin]}
    if {$pin_name eq ""} {
        catch {set pin_name [get_property NAME $pin]}
    }
    if {$pin_name eq ""} {
        return ""
    }
    set parts [split $pin_name "/"]
    return [lindex $parts end]
}

proc get_bus_width {obj left_prop right_prop} {
    set left 0
    set right 0
    catch {set left [get_property $left_prop $obj]}
    catch {set right [get_property $right_prop $obj]}
    if {$left eq ""} {
        set left 0
    }
    if {$right eq ""} {
        set right $left
    }
    return [expr {abs(int($left) - int($right)) + 1}]
}

proc get_relative_depth {path} {
    set parts [split $path "/"]
    set clean_parts {}
    foreach part $parts {
        if {$part ne ""} {
            lappend clean_parts $part
        }
    }
    return [llength $clean_parts]
}

proc matches_module_filter {cell_name use_filter filter_list} {
    if {!$use_filter} {
        return 1
    }
    foreach filter_name $filter_list {
        if {$filter_name eq ""} {
            continue
        }
        if {[string match "*$filter_name*" $cell_name]} {
            return 1
        }
    }
    return 0
}

proc maybe_add_clock_alias {net clocks_var seen_clock_var} {
    upvar 1 $clocks_var clocks
    upvar 1 $seen_clock_var seen_clock

    set full_path ""
    set logical_name ""
    set net_type ""
    catch {set full_path [get_property FULL_NAME $net]}
    catch {set logical_name [get_property NAME $net]}
    catch {set net_type [string toupper [get_property TYPE $net]]}

    if {$full_path eq "" || $logical_name eq ""} {
        return
    }

    set looks_like_clock 0
    if {$net_type eq "CLOCK"} {
        set looks_like_clock 1
    }
    if {!$looks_like_clock} {
        if {[string match -nocase "*clk*" $logical_name] || [string match -nocase "*clock*" $logical_name]} {
            set looks_like_clock 1
        }
    }
    if {!$looks_like_clock} {
        return
    }

    if {[info exists seen_clock($full_path)]} {
        return
    }
    set seen_clock($full_path) 1
    lappend clocks [dict create \
        logical_name $logical_name \
        waveform_path $full_path \
        period_ns 0.0 \
    ]
}

proc first_pin_matching {pins patterns} {
    foreach pin $pins {
        set leaf_name [pin_leaf_name $pin]
        foreach pattern $patterns {
            if {[string match $pattern $leaf_name]} {
                return $pin
            }
        }
    }
    return ""
}

proc get_clock_name_from_cell {cell} {
    set pins [get_pins -quiet -of_objects $cell -filter {DIRECTION == "IN"}]
    foreach pin $pins {
        set leaf_name [pin_leaf_name $pin]
        if {![string match "C*" $leaf_name] && ![string match "CLK*" $leaf_name]} {
            continue
        }
        set nets [get_nets -quiet -of_objects $pin]
        if {[llength $nets] == 0} {
            continue
        }
        set clk_name ""
        catch {set clk_name [get_property NAME [lindex $nets 0]]}
        if {$clk_name ne ""} {
            return $clk_name
        }
    }
    return ""
}

proc register_edge {edges_var edge_seen_var source target inferred_type inferred_by clock clock_edge latency details} {
    upvar 1 $edges_var edges
    upvar 1 $edge_seen_var edge_seen

    if {$source eq "" || $target eq ""} {
        return
    }

    set key [join [list $source $target $inferred_by $clock $latency] "|"]
    if {[info exists edge_seen($key)]} {
        return
    }
    set edge_seen($key) 1

    lappend edges [dict create \
        source $source \
        target $target \
        inferred_type $inferred_type \
        inferred_by $inferred_by \
        clock $clock \
        clock_edge $clock_edge \
        latency_cycles $latency \
        details $details \
    ]
}

proc extract_deps {args} {
    if {![check_vivado_version]} {
        return
    }

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
    while {$i < [llength $args]} {
        set key [lindex $args $i]
        set val [lindex $args [expr {$i + 1}]]
        if {![dict exists $params $key]} {
            error "Unknown parameter: $key"
        }
        dict set params $key $val
        incr i 2
    }

    set top_module [dict get $params -top]
    if {$top_module eq ""} {
        error "Missing required parameter: -top"
    }

    set output_path [dict get $params -output]
    set depth [dict get $params -depth]
    set include_internals [to_bool [dict get $params -include_internals] 1]
    set filter_mods [dict get $params -filter_modules]
    set max_cells [dict get $params -max_cells]
    set max_nets [dict get $params -max_nets]
    set enable_global_clock_fallback [to_bool [dict get $params -enable_global_clock_fallback] 0]

    set filter_list {}
    foreach item [split $filter_mods ","] {
        set trimmed [string trim $item]
        if {$trimmed ne ""} {
            lappend filter_list $trimmed
        }
    }
    set use_filter [expr {[llength $filter_list] > 0}]

    set clocks {}
    set boundary_ports {}
    set modules {}
    set edges {}

    array set selected_cell_names {}
    array set cell_ref_names {}
    array set cell_clock_names {}
    array set seen_nets {}
    array set seen_clock_paths {}
    array set edge_seen {}

    stage_begin "Extract top-level ports"
    set top_ports [get_ports -quiet]
    foreach port $top_ports {
        set direction [get_property DIRECTION $port]
        set full_path [get_property FULL_NAME $port]
        if {$full_path eq ""} {
            continue
        }
        set kind "output_port"
        if {$direction eq "IN"} {
            set kind "input_port"
        }
        lappend boundary_ports [dict create \
            path $full_path \
            direction $direction \
            width [get_bus_width $port LEFT RIGHT] \
            kind $kind \
        ]
    }
    stage_end "[llength $boundary_ports] ports"

    stage_begin "Select hierarchy cells"
    set all_cells [get_cells -quiet -hierarchical -filter {IS_PRIMITIVE == "FALSE" && PRIMITIVE_LEVEL != "MACRO"}]
    set filtered_cells {}
    foreach cell $all_cells {
        set cell_name [get_property NAME $cell]
        if {[get_relative_depth $cell_name] > $depth} {
            continue
        }
        if {![matches_module_filter $cell_name $use_filter $filter_list]} {
            continue
        }
        lappend filtered_cells $cell
    }
    set filtered_cells [trim_list $filtered_cells $max_cells "hierarchy cells"]
    foreach cell $filtered_cells {
        set cell_name [get_property NAME $cell]
        set ref_name [get_property REF_NAME $cell]
        set selected_cell_names($cell_name) 1
        set cell_ref_names($cell_name) $ref_name
        set cell_clock_names($cell_name) [get_clock_name_from_cell $cell]

        set pins [get_pins -quiet -of_objects $cell -filter {IS_HIERARCHICAL == "FALSE"}]
        set ports {}
        foreach pin $pins {
            set pin_name [pin_leaf_name $pin]
            if {$pin_name eq ""} {
                continue
            }
            set pin_dir [get_property DIRECTION $pin]
            lappend ports [dict create \
                name $pin_name \
                direction $pin_dir \
                width [get_bus_width $pin BUS_LEFT BUS_RIGHT] \
            ]
        }
        lappend modules [dict create \
            instance $cell_name \
            canonical $cell_name \
            module_type $ref_name \
            ports $ports \
        ]
    }
    stage_end "[llength $filtered_cells] selected cells, [llength $modules] modules"

    stage_begin "Collect candidate nets"
    set candidate_nets {}
    foreach cell $filtered_cells {
        set pins [get_pins -quiet -of_objects $cell -filter {IS_HIERARCHICAL == "FALSE"}]
        if {[llength $pins] == 0} {
            continue
        }
        set nets [get_nets -quiet -of_objects $pins]
        foreach net $nets {
            set net_full_name ""
            set net_type ""
            catch {set net_full_name [get_property FULL_NAME $net]}
            catch {set net_type [string toupper [get_property TYPE $net]]}
            if {$net_full_name eq ""} {
                continue
            }
            if {$net_type eq "POWER" || $net_type eq "GROUND"} {
                continue
            }
            if {[info exists seen_nets($net_full_name)]} {
                continue
            }
            set seen_nets($net_full_name) 1
            lappend candidate_nets $net
            maybe_add_clock_alias $net clocks seen_clock_paths
        }
    }

    set port_nets [get_nets -quiet -of_objects $top_ports]
    foreach net $port_nets {
        set net_full_name ""
        set net_type ""
        catch {set net_full_name [get_property FULL_NAME $net]}
        catch {set net_type [string toupper [get_property TYPE $net]]}
        if {$net_full_name eq ""} {
            continue
        }
        if {$net_type eq "POWER" || $net_type eq "GROUND"} {
            continue
        }
        if {[info exists seen_nets($net_full_name)]} {
            continue
        }
        set seen_nets($net_full_name) 1
        lappend candidate_nets $net
        maybe_add_clock_alias $net clocks seen_clock_paths
    }

    if {$enable_global_clock_fallback && [llength $clocks] == 0} {
        puts "INFO: local clock detection found no clocks, trying bounded global fallback"
        set global_nets [trim_list [get_nets -quiet -hierarchical] $max_nets "global clock scan nets"]
        foreach net $global_nets {
            maybe_add_clock_alias $net clocks seen_clock_paths
        }
    }

    set candidate_nets [trim_list $candidate_nets $max_nets "candidate nets"]
    stage_end "[llength $candidate_nets] nets, [llength $clocks] clocks"

    stage_begin "Extract net edges"
    foreach net $candidate_nets {
        set drivers {}
        set loads {}

        set net_pins [get_pins -quiet -of_objects $net]
        foreach pin $net_pins {
            set pin_full_name [get_property FULL_NAME $pin]
            set pin_dir [string toupper [get_property DIRECTION $pin]]
            if {$pin_full_name eq "" || ($pin_dir ne "IN" && $pin_dir ne "OUT")} {
                continue
            }

            set owner_cells [get_cells -quiet -of_objects $pin]
            if {[llength $owner_cells] == 0} {
                continue
            }
            set owner_cell [lindex $owner_cells 0]
            set owner_name [get_property NAME $owner_cell]
            if {![info exists selected_cell_names($owner_name)]} {
                continue
            }
            set owner_type $cell_ref_names($owner_name)
            set pin_info [dict create \
                signal $pin_full_name \
                pin_name [pin_leaf_name $pin] \
                owner_name $owner_name \
                owner_type $owner_type \
            ]
            if {$pin_dir eq "OUT"} {
                lappend drivers $pin_info
            } else {
                lappend loads $pin_info
            }
        }

        set net_ports [get_ports -quiet -of_objects $net]
        foreach port $net_ports {
            set port_full_name [get_property FULL_NAME $port]
            set port_dir [string toupper [get_property DIRECTION $port]]
            if {$port_full_name eq "" || ($port_dir ne "IN" && $port_dir ne "OUT")} {
                continue
            }
            set port_info [dict create \
                signal $port_full_name \
                pin_name $port_full_name \
                owner_name $top_module \
                owner_type "__PORT__" \
            ]
            if {$port_dir eq "IN"} {
                lappend drivers $port_info
            } else {
                lappend loads $port_info
            }
        }

        foreach driver $drivers {
            set drv_signal [dict get $driver signal]
            set drv_type [dict get $driver owner_type]
            foreach load $loads {
                set ld_signal [dict get $load signal]
                set ld_type [dict get $load owner_type]
                set ld_owner [dict get $load owner_name]
                set ld_pin_name [dict get $load pin_name]

                set inferred_type "combinational"
                set inferred_by "NET"
                set clock ""
                set clock_edge ""
                set latency 0

                if {[string match "FD*" $ld_type] || [string match "LD*" $ld_type]} {
                    if {[string match "D*" $ld_pin_name]} {
                        set inferred_type "sequential"
                        set inferred_by "FF"
                        set latency 1
                        if {[info exists cell_clock_names($ld_owner)]} {
                            set clock $cell_clock_names($ld_owner)
                        }
                        set clock_edge "posedge"
                    } elseif {[string match "CE*" $ld_pin_name] || [string match "EN*" $ld_pin_name]} {
                        set inferred_type "control"
                        set inferred_by "FF_CE"
                    } elseif {[string match "R*" $ld_pin_name] || [string match "S*" $ld_pin_name]} {
                        set inferred_type "control"
                        set inferred_by "FF_RST"
                    }
                } elseif {[string match "RAMB*" $ld_type] || [string match "RAMB*" $drv_type]} {
                    set inferred_type "memory"
                    set inferred_by "BRAM"
                    set latency 2
                }

                register_edge \
                    edges edge_seen \
                    $drv_signal \
                    $ld_signal \
                    $inferred_type \
                    $inferred_by \
                    $clock \
                    $clock_edge \
                    $latency \
                    "$ld_type: $ld_owner"
            }
        }
    }
    stage_end "[llength $edges] edges after net walk"

    if {$include_internals} {
        stage_begin "Extract internal FF and BRAM edges"
        foreach cell $filtered_cells {
            set cell_name [get_property NAME $cell]
            set ref_name $cell_ref_names($cell_name)
            set pins [get_pins -quiet -of_objects $cell -filter {IS_HIERARCHICAL == "FALSE"}]

            if {[string match "FD*" $ref_name] || [string match "LD*" $ref_name]} {
                set d_pin [first_pin_matching $pins {"D"}]
                set q_pin [first_pin_matching $pins {"Q"}]
                if {$d_pin ne "" && $q_pin ne ""} {
                    set d_sig [get_property FULL_NAME $d_pin]
                    set q_sig [get_property FULL_NAME $q_pin]
                    set clk_name ""
                    if {[info exists cell_clock_names($cell_name)]} {
                        set clk_name $cell_clock_names($cell_name)
                    }
                    register_edge \
                        edges edge_seen \
                        $d_sig \
                        $q_sig \
                        "sequential" \
                        "FF" \
                        $clk_name \
                        "posedge" \
                        1 \
                        "$ref_name: $cell_name"
                }
            }

            if {[string match "RAMB*" $ref_name]} {
                set addr_pins {}
                set dout_pins {}
                set en_pins {}
                foreach pin $pins {
                    set leaf_name [pin_leaf_name $pin]
                    if {[string match "ADDR*" $leaf_name]} {
                        lappend addr_pins $pin
                    }
                    if {[string match "DO*" $leaf_name] || [string match "DOUT*" $leaf_name]} {
                        lappend dout_pins $pin
                    }
                    if {[string match "EN*" $leaf_name] || [string match "WE*" $leaf_name]} {
                        lappend en_pins $pin
                    }
                }

                foreach addr_pin $addr_pins {
                    foreach dout_pin $dout_pins {
                        register_edge \
                            edges edge_seen \
                            [get_property FULL_NAME $addr_pin] \
                            [get_property FULL_NAME $dout_pin] \
                            "memory" \
                            "BRAM" \
                            "" \
                            "posedge" \
                            2 \
                            "$ref_name: $cell_name"
                    }
                }
                foreach en_pin $en_pins {
                    foreach dout_pin $dout_pins {
                        register_edge \
                            edges edge_seen \
                            [get_property FULL_NAME $en_pin] \
                            [get_property FULL_NAME $dout_pin] \
                            "control" \
                            "BRAM_EN" \
                            "" \
                            "" \
                            0 \
                            "$ref_name: $cell_name"
                    }
                }
            }
        }
        stage_end "[llength $edges] total edges"
    }

    stage_begin "Write deps_raw.json"
    write_deps_json $output_path $top_module $depth $clocks $boundary_ports $modules $edges
    stage_end $output_path

    puts "Done. Extracted [llength $clocks] clocks, [llength $boundary_ports] boundary ports, [llength $modules] modules, [llength $edges] edges."
}

proc json_escape {str} {
    regsub -all {\\} $str {\\\\} str
    regsub -all {"} $str {\\"} str
    regsub -all {\n} $str {\\n} str
    regsub -all {\r} $str {\\r} str
    regsub -all {\t} $str {\\t} str
    regsub -all {[\x00-\x1f]} $str {} str
    return $str
}

proc write_deps_json {output_path top_module depth clocks boundary_ports modules edges} {
    set f [open $output_path "w"]
    set extract_time [clock format [clock seconds] -format "%Y-%m-%dT%H:%M:%S" -gmt true]

    puts $f "\{"
    puts $f "  \"format_version\": \"1.0\","
    puts $f "  \"extractor\": \"vivado_tcl\","
    puts $f "  \"extract_time\": \"[json_escape $extract_time]\","
    puts $f "  \"top_module\": \"[json_escape $top_module]\","
    puts $f "  \"depth\": $depth,"

    # clocks
    puts $f "  \"clocks\": \["
    set first true
    foreach clk $clocks {
        if {!$first} {
            puts $f ","
        }
        set first false
        set ln [json_escape [dict get $clk logical_name]]
        set wp [json_escape [dict get $clk waveform_path]]
        set pn [dict get $clk period_ns]
        puts -nonewline $f "    \{\"logical_name\": \"$ln\", \"waveform_path\": \"$wp\", \"period_ns\": $pn\}"
    }
    puts $f ""
    puts $f "  \],"

    # boundary_ports
    puts $f "  \"boundary_ports\": \["
    set first true
    foreach bp $boundary_ports {
        if {!$first} {
            puts $f ","
        }
        set first false
        set p [json_escape [dict get $bp path]]
        set d [dict get $bp direction]
        set w [dict get $bp width]
        set k [dict get $bp kind]
        puts -nonewline $f "    \{\"path\": \"$p\", \"direction\": \"$d\", \"width\": $w, \"kind\": \"$k\"\}"
    }
    puts $f ""
    puts $f "  \],"

    # modules
    puts $f "  \"modules\": \["
    set first true
    foreach mod $modules {
        if {!$first} {
            puts $f ","
        }
        set first false
        puts $f "    \{"
        puts $f "      \"instance\": \"[json_escape [dict get $mod instance]]\","
        puts $f "      \"canonical\": \"[json_escape [dict get $mod canonical]]\","
        puts $f "      \"module_type\": \"[json_escape [dict get $mod module_type]]\","
        puts -nonewline $f "      \"ports\": \["
        set port_first true
        foreach port [dict get $mod ports] {
            if {!$port_first} {
                puts -nonewline $f ", "
            }
            set port_first false
            set pn [json_escape [dict get $port name]]
            set pd [dict get $port direction]
            set pw [dict get $port width]
            puts -nonewline $f "\{\"name\": \"$pn\", \"direction\": \"$pd\", \"width\": $pw\}"
        }
        puts $f "\]"
        puts -nonewline $f "    \}"
    }
    puts $f ""
    puts $f "  \],"

    # edges
    puts $f "  \"edges\": \["
    set first true
    foreach e $edges {
        if {!$first} {
            puts $f ","
        }
        set first false
        set clk_val "null"
        set edge_val "null"
        if {[dict get $e clock] ne ""} {
            set clk_val "\"[json_escape [dict get $e clock]]\""
        }
        if {[dict get $e clock_edge] ne ""} {
            set edge_val "\"[json_escape [dict get $e clock_edge]]\""
        }
        set src [json_escape [dict get $e source]]
        set tgt [json_escape [dict get $e target]]
        set it [dict get $e inferred_type]
        set ib [dict get $e inferred_by]
        set lc [dict get $e latency_cycles]
        set dt [json_escape [dict get $e details]]
        puts -nonewline $f "    \{\"source\": \"$src\", \"target\": \"$tgt\", \"inferred_type\": \"$it\", \"inferred_by\": \"$ib\", \"clock\": $clk_val, \"clock_edge\": $edge_val, \"latency_cycles\": $lc, \"details\": \"$dt\"\}"
    }
    puts $f ""
    puts $f "  \]"
    puts $f "\}"
    close $f
}

} ;# end namespace

proc extract_deps {args} {
    ::deps_extractor::extract_deps {*}$args
}
