#!/usr/bin/env python3
import json
import sys
import z3

results = []
def check(name, conditions, expected):
    solver = z3.Solver()
    solver.add(*conditions)
    result = solver.check()
    assert result == expected, (name, result, expected)
    results.append({'name':name, 'expected':str(expected), 'actual':str(result)})
    print(name + ': ' + str(result))

Owner = z3.Datatype('OptionalOwner')
Owner.declare('none')
Owner.declare('some', ('span', z3.IntSort()), ('owner', z3.IntSort()))
Owner = Owner.create()
selected = z3.Const('selected_owner', Owner)
output = z3.Int('explicit_owner')
nat = z3.Implies(Owner.is_some(selected), z3.And(Owner.span(selected) >= 0, Owner.owner(selected) >= 0))
original = z3.If(Owner.is_some(selected), Owner.owner(selected), 0)
explicit = z3.Or(z3.And(selected == Owner.none, output == 0),
                 z3.And(Owner.is_some(selected), output == Owner.owner(selected)))
check('owner_none_premise', [nat, explicit, selected == Owner.none], z3.sat)
check('owner_some_premise', [nat, explicit, Owner.is_some(selected), Owner.owner(selected) == 3], z3.sat)
check('owner_match_equivalence', [nat, explicit, output != original], z3.unsat)
Color = z3.Datatype('OptionalColor')
Color.declare('none')
Color.declare('some', ('value', z3.IntSort()))
Color = Color.create()
selected = z3.Const('selected_color', Color)
output = z3.Int('explicit_color')
nat = z3.Implies(Color.is_some(selected), Color.value(selected) >= 0)
original = z3.If(Color.is_some(selected), Color.value(selected), 256)
explicit = z3.Or(z3.And(selected == Color.none, output == 256),
                 z3.And(Color.is_some(selected), output == Color.value(selected)))
check('color_none_premise', [nat, explicit, selected == Color.none], z3.sat)
check('color_some_premise', [nat, explicit, Color.is_some(selected), Color.value(selected) == 0], z3.sat)
check('color_match_equivalence', [nat, explicit, output != original], z3.unsat)
check('exhaustion_stays_invalid_256', [nat, explicit, selected == Color.none, output != 256], z3.unsat)
check('last_byte_255_retained', [nat, explicit, selected == Color.some(255), output == 255], z3.sat)
print(json.dumps({'passed':True, 'z3_version':z3.get_version_string(), 'checks':results}, indent=2))
