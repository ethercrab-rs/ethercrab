set datafile separator comma

plot \
'dc-align.csv' using 1:4 with lines, \
'dc-align.csv' using 1:5 with lines dashtype 2, \
'dc-align.csv' using 1:6 with lines, \
'dc-align.csv' using 1:7 with lines dashtype 2, \
'dc-align.csv' using 1:8 with lines, \
'dc-align.csv' using 1:9 with lines dashtype 2, \
'dc-align.csv' using 1:10 with lines, \
'dc-align.csv' using 1:11 with lines dashtype 2, \
'dc-align.csv' using 1:12 with lines, \
'dc-align.csv' using 1:13 with lines dashtype 2, \
'dc-align.csv' using 1:14 with lines, \
'dc-align.csv' using 1:15 with lines dashtype 2
