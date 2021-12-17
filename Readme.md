# UMDH Analyzer
[UMDH](https://docs.microsoft.com/en-us/windows-hardware/drivers/debugger/umdh) tool dumps heap snapshot of a process.
This helps in finding memory leaks.

## You need this additional tool if:
1. You can only reproduce the memory leak in production.
1. You don't know which workflow is triggering the memory leak.
1. Memory leak is slow.
1. You have many workflows, objects that are used and have varying lifetime.
1. You use Managed memory language like C#, Java with native Interop. Native objects may get deleted during GC run, which can take a while to run.
1. A combination of above cases.

In production, depending upon your traffic, your memory object snapshot will differ from time to time.
<br>Since, UMDH does not know your object allocation pattern and traffic, you will mostly see your hot path objects in UMDH diff log file.
<br>Because of these false positives, a lot of time get wasted, if we head in wrong direction.

BackTrace* is unique id for each call stack that allocates memory.

## What does this tool do differently ?
* Luckily, UMDH prints memory address of a specific BackTrace*.
* If we see that for a BackTrace*, same memory address is present in all umdh log files, it means that
   * either this allocation is part of some global object. If number of allocations for BackTrace* is high, it is cause of concern.
   * either this memory address is always returned for memory allocation of a specific call stack.
        * This is ABA problem. You allocate memory. You release memory. You allocate memory again and get back same memory address.
        * This is possible to happen but it can only happen if you allocate/deallocate/allocate very fast.
        * Chances of this can be reduced by either 
           * Taking multiple UMDH snapshots.
           * Looking at dumps and memory contents. If memory contents differ, it is ABA and we can ignore this BackTrace* as potential leak.
    * either this is actual memory leak. Yay!!! we found it.

Tool also looks at if there are increasing count of "potential leaks" with new umdh log files i.e. as time progresses.
It flags those BackTrace*.

## How to run tool:
1. Take multile UMDH snapshots. Read [here]() on how to enable UMDH.
2. Pass umdh files to tool in order of oldest to recent log file.
```
> cargo run --release -- .\Umdh_202112071000.txt .\Umdh_202112071530.txt .\Umdh_202112081230.txt .\Umdh_202112091040.txt 
Potential Leaked allocations:
Common backtraces in order of highest numbers:
310,312,1999,BackTrace648E5887
54,56,64,BackTraceF832203
49,49,50,BackTraceF950D83
33,33,41,BackTrace5DEC6F47
33,33,41,BackTrace5DEC6B07
33,33,40,BackTrace5DEC6847
21,21,28,BackTrace5DEC70C7
15,15,21,BackTrace5DEC7247
18,18,20,BackTrace5DEC6C87
1,10,10,BackTrace64B6FB07
1,3,5,BackTrace6232AA87
Potential Variable allocations:
Common backtraces in order of highest numbers:
15,13,33,BackTrace6E4817C7
3,38,31,BackTrace6762DE07
3,28,26,BackTrace6762A307
2,27,22,BackTrace6762A3C7
2,24,21,BackTrace6762E147
5,4,5,BackTrace66833087
3,3,1,BackTrace66810207
```