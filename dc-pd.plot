set datafile separator comma

set term wxt size 1920,1080

set key autotitle columnhead



FILE = "dc-pd.csv"

set multiplot
set size 1.0,0.33

# For time in seconds
# set xlabel 'Elapsed (seconds)'
# set xrange [0.0:0.5]

# For cycle count
set xlabel 'Cycle'
set xrange [700:800]

set ylabel "Microseconds"
set ytics format "%.0f"
# set yrange [0:]

set origin 0.0,0.66
set title "Network times, u32, us"
plot FILE using 2:(column(3) / 1000.0) with lines, \
    for [n=4:18:3] FILE using 2:(column(n) / 1000.0)  with lines

# set origin 0.0,0.33
# set title "Individual device system time"
# plot for [n=4:12:3] FILE using 2:(column(n) / 1000.0)  with lines
set origin 0.0,0.33
set title "System time difference"
plot for [n=6:18:3] FILE using 2:n  with lines

set ylabel "Milliseconds"
set ytics format "%.2f"

set yrange [0:]
set origin 0.0,0.0
set title "Next SYNC0"
plot for [n=5:17:3] FILE using 2:(column(n) / 1000.0/ 1000.0) with lines

exit

set xlabel 'Elapsed (seconds)'

set multiplot                       # multiplot mode (prompt changes to 'multiplot')
set size 1.0, 0.25

set origin 0.0,0.75

set title "Time to next sync0 delta"
plot \
FILE using 1:($9-$10) with lines, \
FILE using 1:($11-$12) with lines, \
FILE using 1:($13-$14) with lines, \
# FILE using 1:($5-$6) with lines, \
# FILE using 1:($7-$8) with lines, \
# FILE using 1:($15-$16) with lines, \

set origin 0.0,0.50

set title "Subdevice local times"
plot \
FILE using 1:10 with lines, \
FILE using 1:12 with lines, \
FILE using 1:14 with lines, \
# FILE using 1:6 with lines, \
# FILE using 1:8 with lines, \
# FILE using 1:16 with lines, \

set origin 0.0,0.25

set title "Next SYNC0"
plot \
FILE using 1:9 with lines, \
FILE using 1:11 with lines, \
FILE using 1:13 with lines, \
# FILE using 1:5 with lines, \
# FILE using 1:7 with lines, \
# FILE using 1:15 with lines, \

set origin 0.0,0.0

set title "Next cycle wait time"

set ylabel 'Time (us)'
plot FILE using 1:($4/1000.) with lines
