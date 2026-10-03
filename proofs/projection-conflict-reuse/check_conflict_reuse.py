"""Independent symbolic check: immutable function congruence and error precedence."""
from z3 import *
source, index = Ints('source index')
scan_ok = Function('scan_ok', IntSort(), IntSort(), BoolSort())
scan_value = Function('scan_value', IntSort(), IntSort(), BitVecSort(16))
scan_error = Function('scan_error', IntSort(), IntSort(), IntSort())
row_ok = Function('row_ok', BitVecSort(16), BoolSort())
row_error = Function('row_error', BitVecSort(16), IntSort())
dep_ok, prefix_ok, has_route, serialize_ok = Bools('dep_ok prefix_ok has_route serialize_ok')
prefix_error, dep_error, serialize_error, dep_state = Ints('prefix_error dep_error serialize_error dep_state')
bytes_fn = Function('serialize', BitVecSort(16), IntSort(), IntSort())
first = scan_value(source,index)
second = scan_value(source,index)
none = BitVecVal(65535,16)
# Tags: 0 successful bytes, 1 exact error payload. Every earlier stage shadows
# later failures. Dependency state can be arbitrary and cannot mutate source.
def run(second_ok, second_value, second_error):
    success = bytes_fn(second_value, dep_state)
    tag = If(prefix_ok, If(scan_ok(source,index), If(row_ok(first), If(dep_ok,
          If(second_ok, If(serialize_ok,0,1),1),1),1),1),1)
    payload = If(prefix_ok, If(scan_ok(source,index), If(row_ok(first), If(dep_ok,
          If(second_ok, If(serialize_ok,success,serialize_error),second_error),dep_error),
          row_error(first)),scan_error(source,index)),prefix_error)
    return tag,payload
old=run(If(has_route,scan_ok(source,index),True),If(has_route,second,none),scan_error(source,index))
new=run(BoolVal(True),If(has_route,first,none),IntVal(0))
s=Solver();s.add(Or(old[0]!=new[0],old[1]!=new[1]));assert s.check()==unsat
print('PASS exact packed conflict/serialization and earliest-error equality: UNSAT')
# Establish that the proof would reject a mutated source/index or hoisting the
# dependency failure ahead of the original first scan.
other_source=Int('other_source')
bad=run(scan_ok(other_source,index),If(has_route,scan_value(other_source,index),none),scan_error(other_source,index))
s=Solver();s.add(prefix_ok,scan_ok(source,index),row_ok(first),dep_ok,has_route,serialize_ok,
    Or(old[0]!=bad[0],old[1]!=bad[1]));assert s.check()==sat
print('PASS changed-source negative control: SAT')
s=Solver();s.add(prefix_ok,Not(scan_ok(source,index)),Not(dep_ok),scan_error(source,index)!=dep_error,
    old[1]!=dep_error);assert s.check()==sat
print('PASS moved-dependency negative control: SAT')
print('Z3',get_version_string())
