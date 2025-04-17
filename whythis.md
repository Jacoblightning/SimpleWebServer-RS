# Why should I use this?
IDK, you probably shouldn't.
It's my first rust project and there are better alternatives out there.
(Although just wait until we get down to the data)

# The data:
So, you want to see how it stacks up?

Well, truthfully, IDK.
I tested it with `ab` (ApacheBench) and it apparently returned very good results,
but I probably just used `ab` wrong.

If you want to see them anyway, here they are:

(These tests were run with a `v1.0.0` server binary running with arguments: `-r0`)

Serving a simple Hello, World HTML file:
```
Document Path:          /
Document Length:        95 bytes

Concurrency Level:      10
Time taken for tests:   6.291 seconds
Complete requests:      100000
Failed requests:        0
Total transferred:      11200000 bytes
HTML transferred:       9500000 bytes
Requests per second:    15895.93 [#/sec] (mean)
Time per request:       0.629 [ms] (mean)
Time per request:       0.063 [ms] (mean, across all concurrent requests)
Transfer rate:          1738.62 [Kbytes/sec] received

Connection Times (ms)
              min  mean[+/-sd] median   max
Connect:        0    0   0.1      0       2
Processing:     0    0   0.1      0       2
Waiting:        0    0   0.0      0       2
Total:          0    1   0.1      1       3

Percentage of the requests served within a certain time (ms)
  50%      1
  66%      1
  75%      1
  80%      1
  90%      1
  95%      1
  98%      1
  99%      1
 100%      3 (longest request)
```

Serving [CBLT](https://crates.io/crates/cblt)'s logo (against their benchmark):
```
Document Path:          /logo.png
Document Length:        23911 bytes

Concurrency Level:      1000
Time taken for tests:   0.243 seconds
Complete requests:      3000
Failed requests:        0
Total transferred:      71784000 bytes
HTML transferred:       71733000 bytes
Requests per second:    12346.19 [#/sec] (mean)
Time per request:       80.997 [ms] (mean)
Time per request:       0.081 [ms] (mean, across all concurrent requests)
Transfer rate:          288495.67 [Kbytes/sec] received

Connection Times (ms)
              min  mean[+/-sd] median   max
Connect:        0    9   4.6      7      23
Processing:     3   10   3.3      9      29
Waiting:        0    4   2.9      3      15
Total:         10   18   6.2     16      40

Percentage of the requests served within a certain time (ms)
  50%     16
  66%     16
  75%     16
  80%     16
  90%     29
  95%     37
  98%     38
  99%     39
 100%     40 (longest request)
```
