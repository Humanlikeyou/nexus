#!/bin/sh
cd "$(dirname "$0")/parts" && cat head.html s1-core.js s2-neuro.js s3-hr.js s4-readiness.js s5-sleep.js s6-trends.js s7-money.js tail.html > ../index.html
