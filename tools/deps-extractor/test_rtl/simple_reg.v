// simple_reg.v - minimal test: single register + enable control
// Used to verify Pyverilog extraction correctness

module simple_reg (
    input  wire       clk,
    input  wire       rst_n,
    input  wire       enable,
    input  wire [7:0] data_i,
    output reg  [7:0] data_o
);

always @(posedge clk or negedge rst_n) begin
    if (!rst_n)
        data_o <= 8'h00;
    else if (enable)
        data_o <= data_i;
end

endmodule
