#!/usr/bin/env python3
import json
from collections import OrderedDict
from io import StringIO
from pprint import pprint

filename = 'liqi_admin.json'
data = json.load(open(filename), object_pairs_hook=OrderedDict)
buf = StringIO()
buf.write('syntax = "proto3";\n\n')

data = data['nested']
assert len(data) == 1
package_name = list(data.keys())[0]
buf.write('package {};\n\n'.format(package_name))
data = data[package_name]
data = data['nested']

indent = 0


def write_line(line=''):
    buf.write('{}{}\n'.format(' ' * 2 * indent, line))


def parse_fields(fields):
    for name in fields:
        if 'rule' in fields[name]:
            rule = fields[name]['rule'] + ' '
        else:
            rule = ''
        write_line('{}{} {} = {};'.format(rule, fields[name]['type'], name, fields[name]['id']))


def parse_methods(methods):
    for name in methods:
        write_line(
            'rpc {} ({}) returns ({});'.format(name, methods[name]['requestType'], methods[name]['responseType']))


def parse_values(values):
    for name in values:
        write_line('{} = {};'.format(name, values[name]))


def parse_item(name, item):
    global indent
    if 'fields' in item:
        write_line('message {} {{'.format(name))
        indent += 1
        parse_fields(item['fields'])
    elif 'methods' in item:
        write_line('service {} {{'.format(name))
        indent += 1
        parse_methods(item['methods'])
    elif 'values' in item:
        write_line('enum {} {{'.format(name))
        indent += 1
        parse_values(item['values'])
    else:
        pprint(item)
        raise Exception('Unrecognized Item')
    if 'nested' in item:
        assert len(item) == 2
        nested = item['nested']
        for child in nested:
            parse_item(child, nested[child])

    indent -= 1
    write_line('}\n')


for name in data:
    parse_item(name, data[name])

with open('protocol_admin.proto', 'w') as f:
    f.write(buf.getvalue())
