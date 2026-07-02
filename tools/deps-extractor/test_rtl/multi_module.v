module channel (
    input  wire       clk,
    input  wire       enable,
    input  wire [7:0] data_i,
    output reg  [7:0] data_o
);
always @(posedge clk) begin
    if (enable)
        data_o <= data_i;
end
endmodule

module top_multi (
    input  wire       clk,
    input  wire       ch0_enable,
    input  wire       ch1_enable,
    input  wire [7:0] ch0_data_i,
    input  wire [7:0] ch1_data_i,
    output wire [7:0] ch0_data_o,
    output wire [7:0] ch1_data_o
);

channel u_ch0 (
    .clk(clk),
    .enable(ch0_enable),
    .data_i(ch0_data_i),
    .data_o(ch0_data_o)
);

channel u_ch1 (
    .clk(clk),
    .enable(ch1_enable),
    .data_i(ch1_data_i),
    .data_o(ch1_data_o)
);

endmodule