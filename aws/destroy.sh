#!/bin/bash
set -e

# Configuration
CLUSTER_NAME="sol-agg-cluster"
SERVICE_NAME="sol-agg-service"
REPO_NAME="solana-aggregator"
LOG_GROUP="/ecs/sol-aggregator"
TASK_FAMILY="sol-agg-task"
REGION="eu-central-1"

echo "⚠️  WARNING: This script will delete the AWS resources for the Solana Aggregator."
echo "   Region: $REGION"
echo "   Cluster: $CLUSTER_NAME"
echo "   Service: $SERVICE_NAME"
echo ""
read -p "Are you sure you want to proceed? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 1
fi

# 1. Delete ECS Service
echo ""
echo "🛑 Step 1: Deleting ECS Service..."
# First scale to 0
aws ecs update-service --cluster $CLUSTER_NAME --service $SERVICE_NAME --desired-count 0 --region $REGION > /dev/null
echo "   Service scaled to 0..."
# Delete service
aws ecs delete-service --cluster $CLUSTER_NAME --service $SERVICE_NAME --force --region $REGION > /dev/null
echo "   Service deleted."

# 2. Deregister Task Definitions
echo ""
echo "🗑️  Step 2: Deregistering Task Definitions..."
# List all active task definitions for the family
ARNS=$(aws ecs list-task-definitions --family-prefix $TASK_FAMILY --status ACTIVE --region $REGION --query "taskDefinitionArns[]" --output text)
for arn in $ARNS; do
    echo "   Deregistering: $arn"
    aws ecs deregister-task-definition --task-definition $arn --region $REGION > /dev/null
done

# 3. Delete ECR Repository
echo ""
echo "📦 Step 3: Deleting ECR Repository..."
aws ecr delete-repository --repository-name $REPO_NAME --force --region $REGION > /dev/null
echo "   Repository $REPO_NAME deleted."

# 4. Delete CloudWatch Log Group
echo ""
echo "QK Step 4: Deleting CloudWatch Logs..."
aws logs delete-log-group --log-group-name $LOG_GROUP --region $REGION || echo "   Log group not found or already deleted."

# 5. Delete ECS Cluster
echo ""
echo "🖥️  Step 5: Deleting ECS Cluster..."
# Only delete if empty
aws ecs delete-cluster --cluster $CLUSTER_NAME --region $REGION > /dev/null
echo "   Cluster $CLUSTER_NAME deleted."

echo ""
echo "✅ Stateless resources deleted."
echo ""
echo "⚠️  MANUAL DELETION REQUIRED FOR STATEFUL RESOURCES:"
echo "   These resources were likely created manually and are not deleted by this script to prevent data loss."
echo ""
echo "   1. Load Balancer (ALB): Check EC2 -> Load Balancers (likely named 'sol-agg-alb')"
echo "   2. Target Groups: Check EC2 -> Target Groups"
echo "   3. RDS Database: Check RDS -> Databases (likely named 'sol-agg-db')"
echo "   4. Networking: VPC, Subnets, Security Groups created for this project"
echo ""
