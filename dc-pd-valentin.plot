set datafile separator comma

set term wxt size 1920,1080

set key autotitle columnhead



FILE = "dc-pd.csv"

set multiplot
set size 1.0,0.25

# For time in seconds
# set xlabel 'Elapsed (seconds)'
# set xrange [0.0:10.0]

# For cycle count
set xlabel 'Cycle'
# set xrange [700:800]

# Milliseconds
# set yrange [0:10]
set origin 0.0,0.75
set title "SubDevice system time u32"
plot for [n=4:23:4] FILE using 2:(column(n)) with lines

# Milliseconds
# set yrange [0:10]
set origin 0.0,0.50
set title "SubDevice system time u64"
plot for [n=5:23:4] FILE using 2:(column(n)) with lines

# set yrange [0:3]
set ylabel "Microseconds"
# set logscale y
set origin 0.0,0.25
set title "Time to next SYNC0"
plot for [n=6:23:4] FILE using 2:(column(n)) with lines

# Milliseconds
# set yrange [0:10]
unset logscale y
set ylabel "Nanoseconds"
set origin 0.0,0.0
set title "Next SYNC0 raw value"
plot for [n=7:23:4] FILE using 2:(column(n)) with lines
