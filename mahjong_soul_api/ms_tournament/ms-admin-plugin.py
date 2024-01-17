#!/usr/bin/env python3
import re
import sys

from google.protobuf.compiler import plugin_pb2 as plugin

header = '''# -*- coding: utf-8 -*-
# Generated.  DO NOT EDIT!

import ms_tournament.protocol_admin_pb2 as pb
from ms_tournament.base import MSRPCService
'''

cls_tplt = '''
class {class_name}(MSRPCService):
    version = None
    
    _req = {{
{req_list}
    }}
    _res = {{
{res_list}
    }}

    def get_package_name(self):
        return '{package_name}'

    def get_service_name(self):
        return '{class_name}'

    def get_req_class(self, method):
        return {class_name}._req[method]

    def get_res_class(self, method):
        return {class_name}._res[method]
{func_list}
'''

dict_template = '        \'{method_name}\': pb.{type_name},'

func_template = '''
    async def {func_name}(self, req):
        return await self.call_method('{method_name}', req)'''


def to_snake_case(name):
    s1 = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1_\2', s1).lower()


def generate_code(request, response):
    for proto_file in request.proto_file:
        package_name = proto_file.package

        services = []
        for srv in proto_file.service:
            class_name = srv.name
            req_list = []
            res_list = []
            func_list = []
            for mtd in srv.method:
                method_name = mtd.name
                func_name = to_snake_case(method_name)

                req_name = mtd.input_type.rsplit('.', 1)[-1]
                res_name = mtd.output_type.rsplit('.', 1)[-1]
                req_list.append(dict_template.format(method_name=method_name, type_name=req_name))
                res_list.append(dict_template.format(method_name=method_name, type_name=res_name))
                func_list.append(func_template.format(method_name=method_name, func_name=func_name))

            cls = cls_tplt.format(package_name=package_name,
                                  class_name=class_name,
                                  req_list='\n'.join(req_list),
                                  res_list='\n'.join(res_list),
                                  func_list='\n'.join(func_list))
            services.append(cls)

        body = '\n'.join(services)
        src = header + '\n' + body
        f = response.file.add()
        f.name = 'rpc.py'
        f.content = src


if __name__ == '__main__':
    # Read request message from stdin
    data = sys.stdin.buffer.read()

    # Parse request
    request = plugin.CodeGeneratorRequest()
    request.ParseFromString(data)

    # Create response
    response = plugin.CodeGeneratorResponse()

    # Generate code
    generate_code(request, response)

    # Serialise response message
    output = response.SerializeToString()

    # Write to stdout
    sys.stdout.buffer.write(output)
