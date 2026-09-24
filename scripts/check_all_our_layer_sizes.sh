#!/bin/bash

# You need to have some AWS profile configured in order to use this script.
# So unless you use the default profile, you need to do `export AWS_PROFILE=something` before runing the script.
# The layers are public so the profile can be for any AWS account. 

export AWS_REGION=eu-west-1

layer=arn:aws:lambda:eu-west-1:625240141681:layer:coralogix-aws-lambda-telemetry-exporter-x86_64:37
echo $layer
${BASH_SOURCE%/*}/check_layer_size.sh $layer
echo ""

layer=arn:aws:lambda:eu-west-1:625240141681:layer:coralogix-aws-lambda-telemetry-exporter-arm64:37
echo $layer
${BASH_SOURCE%/*}/check_layer_size.sh $layer
echo ""

layer=arn:aws:lambda:eu-west-1:625240141681:layer:coralogix-python-wrapper-and-exporter-x86_64:26
echo $layer
${BASH_SOURCE%/*}/check_layer_size.sh $layer
echo ""

layer=arn:aws:lambda:eu-west-1:625240141681:layer:coralogix-python-wrapper-and-exporter-arm64:26
echo $layer
${BASH_SOURCE%/*}/check_layer_size.sh $layer
echo ""

layer=arn:aws:lambda:eu-west-1:625240141681:layer:coralogix-nodejs-wrapper-and-exporter-x86_64:29
echo $layer
${BASH_SOURCE%/*}/check_layer_size.sh $layer
echo ""

layer=arn:aws:lambda:eu-west-1:625240141681:layer:coralogix-nodejs-wrapper-and-exporter-arm64:29
echo $layer
${BASH_SOURCE%/*}/check_layer_size.sh $layer
echo ""

layer=arn:aws:lambda:eu-west-1:625240141681:layer:coralogix-java-wrapper-and-exporter-x86_64:16
echo $layer
${BASH_SOURCE%/*}/check_layer_size.sh $layer
echo ""

layer=arn:aws:lambda:eu-west-1:625240141681:layer:coralogix-java-wrapper-and-exporter-arm64:16
echo $layer
${BASH_SOURCE%/*}/check_layer_size.sh $layer
echo ""