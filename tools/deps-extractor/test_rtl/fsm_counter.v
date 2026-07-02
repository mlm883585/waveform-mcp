module fsm_counter (
    input  wire       clk,
    input  wire       rst_n,
    input  wire       start,
    input  wire [3:0] threshold,
    output reg  [1:0] state,
    output reg  [3:0] count,
    output reg        done
);

localparam S_IDLE = 2'b00;
localparam S_RUN  = 2'b01;
localparam S_DONE = 2'b10;

reg [1:0] next_state;

// Sequential: state register
always @(posedge clk or negedge rst_n) begin
    if (!rst_n) begin
        state <= S_IDLE;
        count <= 4'b0;
    end else begin
        state <= next_state;
        if (state == S_RUN && count < threshold)
            count <= count + 1;
    end
end

// Combinational: next-state logic using case
always @(*) begin
    next_state = state;
    done = 1'b0;
    case (state)
        S_IDLE: begin
            if (start)
                next_state = S_RUN;
            done = 1'b0;
        end
        S_RUN: begin
            if (count >= threshold)
                next_state = S_DONE;
            done = 1'b0;
        end
        S_DONE: begin
            next_state = S_IDLE;
            done = 1'b1;
        end
        default: begin
            next_state = S_IDLE;
            done = 1'b0;
        end
    endcase
end

endmodule